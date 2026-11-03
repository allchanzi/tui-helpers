//! Which-key: a key tree that explains itself while a prefix is pending.
//!
//! The model is generic over the action type and owns no terminal input: the
//! application feeds it characters, receives the outcome, and draws the popup.
//! One tree drives the popup, the help text and the status hint, so they cannot
//! drift apart.
//!
//! Two levels of API:
//! - [`Keymap`] is the pure tree (`press`, `entries`, `summary`).
//! - [`WhichKey`] adds the pending-prefix state: current path, popup delay,
//!   Backspace/Esc handling. Most applications only need this.
use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

/// A key that either runs an action or opens a group of further keys.
#[derive(Clone, Debug)]
pub struct Entry<A> {
    key: char,
    alias: Option<char>,
    label: &'static str,
    kind: Kind<A>,
}

#[derive(Clone, Debug)]
enum Kind<A> {
    Action(A),
    Group(Vec<Entry<A>>),
}

impl<A> Entry<A> {
    pub fn action(key: char, label: &'static str, action: A) -> Self {
        Self {
            key,
            alias: None,
            label,
            kind: Kind::Action(action),
        }
    }
    /// A named sub-menu, shown as `+label` and opened by its key.
    pub fn group(key: char, label: &'static str, children: Vec<Entry<A>>) -> Self {
        Self {
            key,
            alias: None,
            label,
            kind: Kind::Group(children),
        }
    }
    /// A second key for the same entry; the popup shows both as `?/h`.
    pub fn also(mut self, alias: char) -> Self {
        self.alias = Some(alias);
        self
    }
    fn matches(&self, key: char) -> bool {
        self.key == key || self.alias == Some(key)
    }
    fn keys(&self) -> String {
        match self.alias {
            Some(alias) => format!("{}/{}", display_key(self.key), display_key(alias)),
            None => display_key(self.key),
        }
    }
    fn title(&self) -> String {
        match self.kind {
            Kind::Group(_) => format!("+{}", self.label),
            Kind::Action(_) => self.label.to_owned(),
        }
    }
}

fn display_key(key: char) -> String {
    if key == ' ' {
        "Space".into()
    } else {
        key.to_string()
    }
}

/// What a key press did within the tree.
#[derive(Debug, PartialEq, Eq)]
pub enum Step<'a, A> {
    /// Run this action; the prefix is finished.
    Action(&'a A),
    /// A group opened; the path was extended and the popup shows its keys.
    Group,
    /// No such key at this level; the prefix is finished.
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Keymap<A> {
    entries: Vec<Entry<A>>,
}

impl<A> Keymap<A> {
    pub fn new(entries: Vec<Entry<A>>) -> Self {
        Self { entries }
    }

    /// Entries reachable after pressing `path`, or `None` for a stale path.
    pub fn entries(&self, path: &[char]) -> Option<&[Entry<A>]> {
        let mut level = self.entries.as_slice();
        for key in path {
            level = match &level.iter().find(|e| e.matches(*key))?.kind {
                Kind::Group(children) => children,
                Kind::Action(_) => return None,
            };
        }
        Some(level)
    }

    /// Resolve one key. Opening a group appends it to `path`.
    pub fn press(&self, path: &mut Vec<char>, key: char) -> Step<'_, A> {
        let Some(level) = self.entries(path) else {
            return Step::Unknown;
        };
        match level.iter().find(|e| e.matches(key)).map(|e| &e.kind) {
            Some(Kind::Action(action)) => Step::Action(action),
            Some(Kind::Group(_)) => {
                path.push(key);
                Step::Group
            }
            None => Step::Unknown,
        }
    }

    /// One-line summary of a level for help text and status hints.
    pub fn summary(&self, path: &[char]) -> String {
        self.entries(path)
            .unwrap_or_default()
            .iter()
            .map(|e| format!("{} {}", e.keys(), e.title()))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// What a key did to a [`WhichKey`] that was waiting for input.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<A> {
    /// Run this action; the prefix is finished.
    Action(A),
    /// A group opened or the path moved up; keep waiting for keys.
    Pending,
    /// The key is not bound at this level; the prefix is finished.
    Unknown,
    /// Backspace at the top level or Esc; the prefix was abandoned.
    Cancelled,
}

/// A [`Keymap`] plus the state of a prefix that is waiting for its next key.
///
/// ```
/// use whichkey::{Entry, Keymap, Outcome, WhichKey};
/// let mut wk = WhichKey::new(Keymap::new(vec![
///     Entry::action('s', "Save", "save"),
///     Entry::group('g', "Git", vec![Entry::action('c', "Commit", "commit")]),
/// ]));
/// wk.open();
/// assert_eq!(wk.press('g'), Outcome::Pending);
/// assert_eq!(wk.press('c'), Outcome::Action("commit"));
/// assert!(!wk.is_open());
/// ```
pub struct WhichKey<A> {
    keymap: Keymap<A>,
    path: Vec<char>,
    since: Option<Instant>,
    delay: Duration,
}

impl<A: Clone> WhichKey<A> {
    /// The popup appears after 300 ms by default, like which-key.nvim.
    pub fn new(keymap: Keymap<A>) -> Self {
        Self {
            keymap,
            path: Vec::new(),
            since: None,
            delay: Duration::from_millis(300),
        }
    }
    /// How long a pending prefix waits before the popup is shown.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
    pub fn keymap(&self) -> &Keymap<A> {
        &self.keymap
    }
    /// Replace the tree (for example after a setting changed the leader key).
    /// A pending prefix is abandoned because its path may no longer exist.
    pub fn set_keymap(&mut self, keymap: Keymap<A>) {
        self.keymap = keymap;
        self.cancel();
    }
    /// Start waiting for the first key after the prefix (for example Space).
    pub fn open(&mut self) {
        self.path.clear();
        self.since = Some(Instant::now());
    }
    pub fn is_open(&self) -> bool {
        self.since.is_some()
    }
    /// Keys pressed inside the tree so far; empty at the top level.
    pub fn path(&self) -> &[char] {
        &self.path
    }
    pub fn cancel(&mut self) {
        self.path.clear();
        self.since = None;
    }
    /// Feed one character. Does nothing useful unless [`is_open`](Self::is_open).
    pub fn press(&mut self, key: char) -> Outcome<A> {
        if !self.is_open() {
            return Outcome::Unknown;
        }
        match self.keymap.press(&mut self.path, key) {
            Step::Group => Outcome::Pending,
            Step::Action(action) => {
                let action = action.clone();
                self.cancel();
                Outcome::Action(action)
            }
            Step::Unknown => {
                self.cancel();
                Outcome::Unknown
            }
        }
    }
    /// Backspace: up one level, or cancel at the top level.
    pub fn back(&mut self) -> Outcome<A> {
        if self.path.pop().is_some() {
            Outcome::Pending
        } else {
            self.cancel();
            Outcome::Cancelled
        }
    }
    /// Whether the popup should be drawn now: always inside a group, and at the
    /// top level only once the delay has passed.
    pub fn visible(&self) -> bool {
        self.since
            .is_some_and(|since| !self.path.is_empty() || since.elapsed() >= self.delay)
    }
    /// Summary of the current level for a status hint.
    pub fn summary(&self) -> String {
        self.keymap.summary(&self.path)
    }
    /// Draw the popup when [`visible`](Self::visible). `root` names the prefix
    /// (for example `"Space"`) and heads the breadcrumb title.
    pub fn draw(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        reserved: u16,
        root: &str,
        colors: Colors,
    ) {
        if !self.visible() {
            return;
        }
        let Some(entries) = self.keymap.entries(&self.path) else {
            return;
        };
        let mut title = vec![root.to_owned()];
        title.extend(self.path.iter().map(|k| k.to_string()));
        draw(frame, area, reserved, &title.join(" › "), entries, colors);
    }
}

/// Colors for the popup; the caller maps its theme onto these.
#[derive(Clone, Copy)]
pub struct Colors {
    pub base: Color,
    pub border: Color,
    pub title: Color,
    pub key: Color,
    pub group: Color,
    pub text: Color,
    pub subtle: Color,
}

/// Draw the popup above the bottom `reserved` rows of `area`.
///
/// `prefix` names what was pressed so far (for example `Space › Inspect`).
pub fn draw<A>(
    frame: &mut Frame<'_>,
    area: Rect,
    reserved: u16,
    prefix: &str,
    entries: &[Entry<A>],
    colors: Colors,
) {
    if entries.is_empty() || area.width < 12 || area.height < 4 {
        return;
    }
    let key_width = entries
        .iter()
        .map(|e| Span::raw(e.keys()).width())
        .max()
        .unwrap_or(1);
    let title_width = entries
        .iter()
        .map(|e| Span::raw(e.title()).width())
        .max()
        .unwrap_or(1);
    // key, " ➜ ", title, two spaces between columns
    let cell = key_width + 3 + title_width + 2;
    let usable = usize::from(area.width.saturating_sub(4));
    let columns = (usable / cell).clamp(1, entries.len());
    let rows = entries.len().div_ceil(columns);
    let hint = if area.width >= 36 {
        "Esc cancel · Backspace back"
    } else {
        "Esc cancel"
    };
    let width = (columns * cell + 2)
        .max(Span::raw(prefix).width() + 4)
        .max(hint.len() + 3)
        .min(usize::from(area.width)) as u16;
    let room = area.height.saturating_sub(reserved);
    let height = ((rows + 3) as u16).min(room);
    if height < 3 {
        return;
    }
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + room - height,
        width,
        height,
    );
    let pad = |text: String, to: usize| {
        let missing = to.saturating_sub(Span::raw(&text).width());
        format!("{text}{}", " ".repeat(missing))
    };
    let mut lines = Vec::new();
    for row in 0..rows {
        let mut spans = vec![Span::raw(" ")];
        for column in 0..columns {
            let Some(entry) = entries.get(column * rows + row) else {
                continue;
            };
            let title_style = if matches!(entry.kind, Kind::Group(_)) {
                Style::new().fg(colors.group).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(colors.text)
            };
            spans.push(Span::styled(
                pad(entry.keys(), key_width),
                Style::new().fg(colors.key).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(" ➜ ", Style::new().fg(colors.subtle)));
            spans.push(Span::styled(
                pad(entry.title(), title_width + 2),
                title_style,
            ));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(Span::styled(
        format!(" {hint}"),
        Style::new().fg(colors.subtle),
    )));
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(colors.text).bg(colors.base))
            .block(
                Block::bordered()
                    .border_style(Style::new().fg(colors.border).bg(colors.base))
                    .title(Span::styled(
                        format!(" {prefix} "),
                        Style::new().fg(colors.title).add_modifier(Modifier::BOLD),
                    )),
            ),
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn map() -> Keymap<&'static str> {
        Keymap::new(vec![
            Entry::action('m', "Map", "map"),
            Entry::action('?', "Help", "help").also('h'),
            Entry::group(
                'i',
                "Inspect",
                vec![
                    Entry::action('a', "Activity", "activity"),
                    Entry::action('t', "Tools", "tools"),
                ],
            ),
        ])
    }

    #[test]
    fn keys_resolve_actions_aliases_groups_and_unknown() {
        let map = map();
        let mut path = Vec::new();
        assert_eq!(map.press(&mut path, 'm'), Step::Action(&"map"));
        assert_eq!(map.press(&mut path, 'h'), Step::Action(&"help"));
        assert_eq!(map.press(&mut path, '?'), Step::Action(&"help"));
        assert_eq!(map.press(&mut path, 'z'), Step::Unknown);
        assert!(path.is_empty());

        assert_eq!(map.press(&mut path, 'i'), Step::Group);
        assert_eq!(path, ['i']);
        assert_eq!(
            map.press(&mut path, 'm'),
            Step::Unknown,
            "level, not global"
        );
        assert_eq!(map.press(&mut path, 't'), Step::Action(&"tools"));
        assert!(map.entries(&['m']).is_none(), "an action has no children");
        assert!(map.entries(&['x']).is_none());
    }

    #[test]
    fn summary_is_generated_from_the_same_tree() {
        let map = map();
        assert_eq!(map.summary(&[]), "m Map · ?/h Help · i +Inspect");
        assert_eq!(map.summary(&['i']), "a Activity · t Tools");
        assert_eq!(map.summary(&['z']), "");
    }

    fn colors() -> Colors {
        Colors {
            base: Color::Black,
            border: Color::Blue,
            title: Color::Magenta,
            key: Color::Yellow,
            group: Color::Blue,
            text: Color::White,
            subtle: Color::Gray,
        }
    }

    fn screen(width: u16, height: u16, path: &[char]) -> String {
        let map = map();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                draw(
                    f,
                    f.area(),
                    2,
                    "Space",
                    map.entries(path).unwrap(),
                    colors(),
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn popup_lists_keys_groups_and_hint_and_stays_above_the_footer() {
        let out = screen(60, 12, &[]);
        for expected in ["Space", "➜ Map", "?/h ➜ Help", "➜ +Inspect", "Esc cancel"] {
            assert!(out.contains(expected), "{expected}\n{out}");
        }
        let rows: Vec<&str> = out.lines().collect();
        assert!(
            rows[10].trim().is_empty() && rows[11].trim().is_empty(),
            "{out}"
        );
        assert!(screen(60, 12, &['i']).contains("a ➜ Activity"));
        assert!(
            !screen(60, 12, &['i']).contains("Map"),
            "group shows only its level"
        );
    }

    #[test]
    fn popup_degrades_on_tiny_screens_without_panicking() {
        for (w, h) in [(0, 0), (5, 3), (12, 4), (20, 6), (80, 24)] {
            let map = map();
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| draw(f, f.area(), 2, "Space", map.entries(&[]).unwrap(), colors()))
                .unwrap();
        }
        // Narrow terminals fold the entries into one column.
        let out = screen(24, 14, &[]);
        assert!(out.contains("➜ Map") && out.contains("➜ +Inspect"), "{out}");
    }

    #[test]
    fn whichkey_tracks_path_delay_back_and_cancel() {
        let mut wk = WhichKey::new(map()).delay(Duration::from_millis(50));
        assert_eq!(
            wk.press('m'),
            Outcome::Unknown,
            "closed prefix ignores keys"
        );
        wk.open();
        assert!(
            wk.is_open() && !wk.visible(),
            "hidden until the delay passes"
        );
        std::thread::sleep(Duration::from_millis(60));
        assert!(wk.visible());
        assert_eq!(wk.press('i'), Outcome::Pending);
        assert_eq!(wk.path(), ['i']);
        assert!(wk.visible());
        assert_eq!(wk.summary(), "a Activity · t Tools");
        assert_eq!(wk.back(), Outcome::Pending);
        assert!(wk.path().is_empty() && wk.is_open());
        assert_eq!(wk.back(), Outcome::Cancelled);
        assert!(!wk.is_open());
        wk.open();
        assert_eq!(wk.press('z'), Outcome::Unknown);
        assert!(!wk.is_open());
        wk.open();
        assert_eq!(wk.press('h'), Outcome::Action("help"));
        wk.open();
        wk.set_keymap(map());
        assert!(!wk.is_open());
    }

    #[test]
    fn whichkey_draws_breadcrumb_inside_groups() {
        let mut wk = WhichKey::new(map()).delay(Duration::from_secs(60));
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        let mut text = |wk: &WhichKey<&'static str>| {
            terminal
                .draw(|f| wk.draw(f, f.area(), 2, "Space", colors()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            (0..12)
                .flat_map(|y| (0..60).map(move |x| (x, y)))
                .map(|(x, y)| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        };
        wk.open();
        assert!(!text(&wk).contains("Map"), "delayed at the top level");
        wk.press('i');
        let out = text(&wk);
        assert!(
            out.contains("Space › i") && out.contains("Activity"),
            "{out}"
        );
    }
}
