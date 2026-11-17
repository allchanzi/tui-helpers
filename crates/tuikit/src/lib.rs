//! Interaction pieces shared by the terminal apps, so that the same key means the
//! same thing everywhere. Application actions stay in the applications.
use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

/// Smallest terminal the apps are laid out for.
pub const MIN_SIZE: (u16, u16) = (40, 10);

/// A movement within a list or a scrolled view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Top,
    Bottom,
    HalfPageUp,
    HalfPageDown,
    PageUp,
    PageDown,
}

impl Nav {
    /// The new position in a list of `len` items, `page` rows high. Stops at the ends.
    pub fn apply(self, index: usize, len: usize, page: usize) -> usize {
        let last = len.saturating_sub(1);
        let half = (page / 2).max(1);
        let page = page.max(1);
        match self {
            Self::Up => index.saturating_sub(1),
            Self::Down => index.saturating_add(1),
            Self::Top => 0,
            Self::Bottom => last,
            Self::HalfPageUp => index.saturating_sub(half),
            Self::HalfPageDown => index.saturating_add(half),
            Self::PageUp => index.saturating_sub(page),
            Self::PageDown => index.saturating_add(page),
        }
        .min(last)
    }
}

/// What a key means to [`NavKeys`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavStep {
    Move(Nav),
    /// The first `g` of `gg`; there is no timeout, any other key cancels it.
    Pending,
    /// Not a navigation key, so the caller handles it.
    Other,
}

/// `j/k`, arrows, `gg/G`, `Home/End`, `Ctrl+u/d`, `Ctrl+b/f` and `PgUp/PgDn`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NavKeys {
    pending_g: bool,
}

impl NavKeys {
    pub fn is_pending(&self) -> bool {
        self.pending_g
    }

    pub fn press(&mut self, key: KeyEvent) -> NavStep {
        let pending = std::mem::take(&mut self.pending_g);
        if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER) {
            return NavStep::Other;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('u') => NavStep::Move(Nav::HalfPageUp),
                KeyCode::Char('d') => NavStep::Move(Nav::HalfPageDown),
                KeyCode::Char('b') => NavStep::Move(Nav::PageUp),
                KeyCode::Char('f') => NavStep::Move(Nav::PageDown),
                _ => NavStep::Other,
            };
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => NavStep::Move(Nav::Up),
            KeyCode::Down | KeyCode::Char('j') => NavStep::Move(Nav::Down),
            KeyCode::Home => NavStep::Move(Nav::Top),
            KeyCode::End | KeyCode::Char('G') => NavStep::Move(Nav::Bottom),
            KeyCode::PageUp => NavStep::Move(Nav::PageUp),
            KeyCode::PageDown => NavStep::Move(Nav::PageDown),
            KeyCode::Char('g') if pending => NavStep::Move(Nav::Top),
            KeyCode::Char('g') => {
                self.pending_g = true;
                NavStep::Pending
            }
            _ => NavStep::Other,
        }
    }
}

/// Neovim's `smartcase`: case matters only when the query contains an uppercase letter.
pub fn smart_contains(text: &str, query: &str) -> bool {
    if query.chars().any(char::is_uppercase) {
        text.contains(query)
    } else {
        text.to_lowercase().contains(&query.to_lowercase())
    }
}

/// The next matching index after `from` (before it when `backwards`), wrapping around.
/// `from` itself is checked last, so it is found when it is the only match.
pub fn next_match(
    len: usize,
    from: usize,
    backwards: bool,
    matches: impl Fn(usize) -> bool,
) -> Option<usize> {
    (1..=len)
        .map(|step| {
            if backwards {
                (from + len - step % len) % len
            } else {
                (from + step) % len
            }
        })
        .find(|index| matches(*index))
}

/// Whether the terminal is large enough for the normal layout.
pub fn fits(width: u16, height: u16) -> bool {
    width >= MIN_SIZE.0 && height >= MIN_SIZE.1
}

/// Draws the too-small notice and returns true when `area` is below [`MIN_SIZE`].
pub fn too_small(frame: &mut Frame<'_>, area: Rect) -> bool {
    if fits(area.width, area.height) {
        return false;
    }
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(format!(
            "Enlarge terminal\nMinimum: {} × {}\n[q]uit",
            MIN_SIZE.0, MIN_SIZE.1
        ))
        .wrap(Wrap { trim: true }),
        area,
    );
    true
}

/// A centered overlay of text that scrolls when it does not fit.
#[derive(Debug, Default)]
pub struct ScrollPopup {
    keys: NavKeys,
    scroll: usize,
    // Written while rendering: only the frame knows how far the text wraps.
    max_scroll: Cell<usize>,
    page: Cell<usize>,
}

impl ScrollPopup {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns true when the key closes the popup. Other keys never reach the
    /// screen behind it.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
        ) {
            return true;
        }
        if let NavStep::Move(nav) = self.keys.press(key) {
            let max = self.max_scroll.get();
            self.scroll = nav
                .apply(self.scroll.min(max), max + 1, self.page.get())
                .min(max);
        }
        false
    }

    pub fn render(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        title: &str,
        lines: Vec<Line<'_>>,
        border: Style,
    ) {
        let width = area.width.min(76);
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        let total = paragraph.line_count(width.saturating_sub(2));
        let height = (total as u16).saturating_add(2).min(area.height);
        let body = height.saturating_sub(2) as usize;
        let max = total.saturating_sub(body);
        self.max_scroll.set(max);
        self.page.set(body.max(1));
        let scroll = self.scroll.min(max);

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(border)
            .title(title.to_string());
        if max > 0 {
            block = block.title_bottom(
                Line::from(format!(
                    " [j/k] scroll • {}/{} ",
                    (scroll + body).min(total),
                    total
                ))
                .alignment(Alignment::Right),
            );
        }
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, rect);
        frame.render_widget(
            paragraph
                .block(block)
                .scroll((scroll.min(u16::MAX as usize) as u16, 0)),
            rect,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn screen(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn gg_needs_two_keys_and_another_key_cancels_it() {
        let mut keys = NavKeys::default();
        assert_eq!(keys.press(key(KeyCode::Char('g'))), NavStep::Pending);
        assert!(keys.is_pending());
        assert_eq!(keys.press(key(KeyCode::Char('g'))), NavStep::Move(Nav::Top));
        keys.press(key(KeyCode::Char('g')));
        assert_eq!(keys.press(key(KeyCode::Char('x'))), NavStep::Other);
        assert!(!keys.is_pending());
        assert_eq!(keys.press(key(KeyCode::Char('g'))), NavStep::Pending);
    }

    #[test]
    fn control_means_paging_and_never_a_plain_motion() {
        let mut keys = NavKeys::default();
        let control = |ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL);
        assert_eq!(keys.press(control('d')), NavStep::Move(Nav::HalfPageDown));
        assert_eq!(keys.press(control('j')), NavStep::Other);
        assert_eq!(
            keys.press(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT)),
            NavStep::Other
        );
    }

    #[test]
    fn movement_stops_at_both_ends() {
        assert_eq!(Nav::Up.apply(0, 5, 2), 0);
        assert_eq!(Nav::Down.apply(4, 5, 2), 4);
        assert_eq!(Nav::Bottom.apply(0, 5, 2), 4);
        assert_eq!(Nav::HalfPageDown.apply(0, 50, 10), 5);
        assert_eq!(Nav::PageDown.apply(45, 50, 10), 49);
        assert_eq!(Nav::Bottom.apply(0, 0, 10), 0);
        assert_eq!(Nav::HalfPageDown.apply(0, 3, 1), 1);
    }

    #[test]
    fn smartcase_matches_neovim() {
        assert!(smart_contains("Fleet", "fle"));
        assert!(smart_contains("Fleet", "Fle"));
        assert!(!smart_contains("fleet", "Fle"));
    }

    #[test]
    fn search_wraps_both_ways_and_finds_the_only_match() {
        let items = ["a", "b", "a", "c"];
        let is_a = |index: usize| items[index] == "a";
        assert_eq!(next_match(4, 0, false, is_a), Some(2));
        assert_eq!(next_match(4, 2, false, is_a), Some(0));
        assert_eq!(next_match(4, 0, true, is_a), Some(2));
        assert_eq!(next_match(4, 3, true, is_a), Some(2));
        assert_eq!(next_match(4, 1, false, |index| index == 1), Some(1));
        assert_eq!(next_match(4, 1, true, |index| index == 1), Some(1));
        assert_eq!(next_match(4, 0, false, |_| false), None);
        assert_eq!(next_match(0, 0, false, |_| true), None);
    }

    #[test]
    fn popup_scrolls_to_the_real_end_and_closes() {
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        let lines = || (0..30).map(|n| Line::from(format!("line-{n}"))).collect();
        let mut popup = ScrollPopup::default();
        terminal
            .draw(|f| popup.render(f, f.area(), " Help ", lines(), Style::new()))
            .unwrap();
        let first = screen(&terminal);
        assert!(first.contains("line-0") && !first.contains("line-29"));
        assert!(first.contains("8/30"));
        assert!(!popup.key(key(KeyCode::Char('G'))));
        terminal
            .draw(|f| popup.render(f, f.area(), " Help ", lines(), Style::new()))
            .unwrap();
        assert!(screen(&terminal).contains("line-29"));
        assert!(popup.key(key(KeyCode::Esc)));
    }

    #[test]
    fn popup_that_fits_has_no_scroll_hint_and_survives_any_size() {
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        let popup = ScrollPopup::default();
        terminal
            .draw(|f| popup.render(f, f.area(), " Help ", vec![Line::from("x")], Style::new()))
            .unwrap();
        assert!(!screen(&terminal).contains("scroll"));
        for (width, height) in [(1, 1), (2, 2), (5, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|f| popup.render(f, f.area(), " Help ", vec![Line::from("x")], Style::new()))
                .unwrap();
        }
    }

    #[test]
    fn too_small_only_below_the_minimum() {
        let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
        let mut small = false;
        terminal.draw(|f| small = too_small(f, f.area())).unwrap();
        assert!(small);
        assert!(screen(&terminal).contains("Enlarge terminal"));
        assert!(fits(40, 10) && !fits(39, 10) && !fits(40, 9));
    }
}
