use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
    text::Span,
    widgets::{Clear, Paragraph},
};
use std::io::{self, Write};
use std::process::{Command, Stdio};

#[derive(Default)]
pub struct TextSelection {
    frame: Option<Buffer>,
    screen: Option<Buffer>,
    terminal_area: Rect,
    start: Position,
    end: Position,
    held: bool,
    dragged: bool,
    notice: Option<String>,
}
impl TextSelection {
    pub fn frozen(&self) -> bool {
        self.frame.is_some()
    }
    pub fn dragged(&self) -> bool {
        self.dragged
    }
    pub fn active(&self) -> bool {
        self.dragged && self.frame.is_some()
    }
    pub fn clear(&mut self) {
        self.frame = None;
        self.held = false;
        self.dragged = false;
        self.notice = None;
    }
    /// Record only the focused content rectangle, excluding its border and other panes.
    // shortcut: selection covers visible cells; add scrollback ranges when selecting across pages.
    pub fn capture(&mut self, screen: &Buffer, area: Rect) {
        let area = area.intersection(screen.area);
        if self.terminal_area != screen.area
            || self.screen.as_ref().is_some_and(|old| old.area != area)
        {
            self.clear();
        }
        self.terminal_area = screen.area;
        if self.frozen() {
            return;
        }
        if area.is_empty() {
            self.screen = None;
            return;
        }
        let mut content = Buffer::empty(area);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                content[(x, y)] = screen[(x, y)].clone();
            }
        }
        self.screen = Some(content);
    }

    /// Consume selection gestures and keys before application actions.
    pub fn event(&mut self, event: &crossterm::event::Event) -> bool {
        use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
        match event {
            Event::Mouse(mouse) => {
                self.mouse(*mouse);
                matches!(
                    mouse.kind,
                    MouseEventKind::Down(MouseButton::Left)
                        | MouseEventKind::Drag(MouseButton::Left)
                        | MouseEventKind::Up(MouseButton::Left)
                )
            }
            Event::Resize(..) => {
                self.clear();
                self.screen = None;
                false
            }
            Event::Key(key) if self.frozen() => {
                if key.kind != KeyEventKind::Press {
                    return true;
                }
                if !self.active() {
                    self.clear();
                    return false;
                }
                if key.code == KeyCode::Esc {
                    self.clear();
                } else if (key.code == KeyCode::Char('y') && key.modifiers.is_empty())
                    || (key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL)
                {
                    let result = self.copy().and_then(|text| copy_text(&text));
                    self.notice = Some(match result {
                        Ok(()) => "Clipboard request sent · [Esc] back".into(),
                        Err(error) => format!("{error} · [Esc] back"),
                    });
                }
                true
            }
            _ => false,
        }
    }

    // Return true only for a completed click; callers retain normal click behavior.
    pub fn mouse(&mut self, event: MouseEvent) -> bool {
        let position = Position::new(event.column, event.row);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.clear();
                self.start = position;
                self.end = position;
                self.held = true;
                if let Some(screen) = &self.screen
                    && screen.area.contains(position)
                {
                    self.frame = Some(screen.clone());
                }
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
                if self.held =>
            {
                if let Some(frame) = &self.frame {
                    self.end = Position::new(
                        position.x.clamp(frame.area.left(), frame.area.right() - 1),
                        position.y.clamp(frame.area.top(), frame.area.bottom() - 1),
                    );
                    self.dragged |= self.end != self.start;
                } else {
                    self.dragged |= position != self.start;
                }
                if event.kind == MouseEventKind::Up(MouseButton::Left) {
                    self.held = false;
                    if !self.dragged {
                        self.clear();
                        return true;
                    }
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => self.clear(),
            _ => {}
        }
        false
    }
    fn bounds(&self) -> (Position, Position) {
        if (self.start.y, self.start.x) <= (self.end.y, self.end.x) {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }
    pub fn text(&self) -> String {
        let Some(frame) = &self.frame else {
            return String::new();
        };
        let (start, end) = self.bounds();
        let mut rows = Vec::new();
        for y in start.y..=end.y {
            let left = if y == start.y {
                start.x
            } else {
                frame.area.left()
            };
            let right = if y == end.y {
                end.x
            } else {
                frame.area.right().saturating_sub(1)
            };
            let mut row = String::new();
            let mut x = frame.area.left();
            while x < frame.area.right() {
                let Some(cell) = frame.cell((x, y)) else {
                    break;
                };
                let width = Span::raw(cell.symbol())
                    .width()
                    .max(1)
                    .min(u16::MAX as usize) as u16;
                if x <= right && x.saturating_add(width) > left {
                    row.push_str(cell.symbol());
                }
                x = x.saturating_add(width);
            }
            rows.push(row.trim_end().to_owned());
        }
        rows.join("\n")
    }
    pub fn paint(&mut self, frame: &mut Frame<'_>, style: Style) -> bool {
        let Some(screen) = &self.frame else {
            return false;
        };
        if self.terminal_area != frame.area() {
            self.clear();
            return false;
        }
        for y in screen.area.top()..screen.area.bottom() {
            for x in screen.area.left()..screen.area.right() {
                frame.buffer_mut()[(x, y)] = screen[(x, y)].clone();
            }
        }
        if self.dragged {
            let (start, end) = self.bounds();
            for y in start.y..=end.y {
                let left = if y == start.y {
                    start.x
                } else {
                    screen.area.left()
                };
                let right = if y == end.y {
                    end.x
                } else {
                    screen.area.right().saturating_sub(1)
                };
                let mut x = screen.area.left();
                while x < screen.area.right() {
                    let width = Span::raw(screen[(x, y)].symbol())
                        .width()
                        .max(1)
                        .min(usize::from(screen.area.right() - x))
                        as u16;
                    if x <= right && x + width > left {
                        for column in x..x + width {
                            frame.buffer_mut()[(column, y)].set_style(style);
                        }
                    }
                    x += width;
                }
            }
            let area = frame.area();
            if area.height > 0 {
                let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
                frame.render_widget(Clear, footer);
                frame.render_widget(
                    Paragraph::new(
                        self.notice
                            .as_deref()
                            .unwrap_or("Text selection · [y/Ctrl+c] copy · [Esc] back"),
                    )
                    .style(style),
                    footer,
                );
            }
        }
        true
    }
    pub fn copy(&self) -> io::Result<String> {
        let text = self.text();
        if text.len() > 65536 {
            return Err(io::Error::other(
                "Selection exceeds 64 KiB; select less text.",
            ));
        }
        Ok(text)
    }
}

pub fn copy_text(text: &str) -> io::Result<()> {
    if text.len() > 65536 {
        return Err(io::Error::other(
            "Selection exceeds 64 KiB; select less text.",
        ));
    }
    let remote =
        std::env::var_os("SSH_TTY").is_some() || std::env::var_os("SSH_CONNECTION").is_some();
    if !remote {
        let tools: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
            ("pbcopy", &[]),
        ];
        for (program, args) in tools {
            if pipe(program, args, text) {
                return Ok(());
            }
        }
    }
    let mut out = io::stdout().lock();
    out.write_all(clipboard_sequence(text, std::env::var_os("TMUX").is_some()).as_bytes())?;
    out.flush()
}

fn clipboard_sequence(text: &str, tmux: bool) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        encoded.push(TABLE[(a >> 2) as usize] as char);
        encoded.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    if tmux {
        format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
    } else {
        sequence
    }
}

fn pipe(program: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    let succeeded = child.wait().is_ok_and(|status| status.success());
    written && succeeded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }
    #[test]
    fn reverse_drag_copies_unicode_without_duplicating_wide_cells_and_click_is_separate() {
        let screen = Buffer::with_lines(["a界žbc", "second"]);
        let mut selection = TextSelection::default();
        selection.capture(&screen, screen.area);
        assert!(!selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 1)));
        selection.mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 1, 0));
        assert!(!selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 1, 0)));
        assert!(selection.active());
        assert_eq!(selection.text(), "界žbc\nsecond");
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        assert!(selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 0, 0)));
        assert!(!selection.frozen());
    }
    #[test]
    fn selection_stays_inside_active_content_and_ignores_neighbours_and_borders() {
        let screen =
            Buffer::with_lines(["LEFT|alpha|RIGHT", "LEFT|bravo|RIGHT", "LEFT|charl|RIGHT"]);
        let area = Rect::new(5, 0, 5, 3);
        let mut selection = TextSelection::default();
        selection.capture(&screen, area);
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 6, 0));
        selection.mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 100, 100));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 100, 100));
        assert_eq!(selection.copy().unwrap(), "lpha\nbravo\ncharl");
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        selection.mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 9, 2));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 9, 2));
        assert!(!selection.active());
        assert_eq!(selection.text(), "");
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 4, 0));
        assert!(selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 4, 0)));
    }

    #[test]
    fn background_updates_preserve_selected_text_and_focus_or_resize_clears_it() {
        let screen = Buffer::with_lines(["before", "second"]);
        let area = Rect::new(0, 0, 6, 1);
        let mut selection = TextSelection::default();
        selection.capture(&screen, area);
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 5, 0));
        selection.capture(&Buffer::with_lines(["after!", "second"]), area);
        assert_eq!(selection.text(), "before");
        selection.capture(&screen, Rect::new(0, 1, 6, 1));
        assert!(!selection.active());
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 1));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 5, 1));
        assert!(selection.active());
        selection.event(&crossterm::event::Event::Resize(80, 20));
        assert!(!selection.frozen());
    }

    #[test]
    fn escape_only_cancels_selection_and_control_c_without_selection_reaches_the_app() {
        use crossterm::event::{Event, KeyCode, KeyEvent};
        let screen = Buffer::with_lines(["hello"]);
        let mut selection = TextSelection::default();
        let control_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!selection.event(&control_c));
        selection.capture(&screen, screen.area);
        selection.event(&Event::Mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            0,
            0,
        )));
        selection.event(&Event::Mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            4,
            0,
        )));
        assert!(selection.event(&Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE
        ))));
        assert!(selection.active());
        assert!(selection.event(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))));
        assert!(!selection.event(&control_c));
    }

    #[test]
    fn clipboard_encoding_is_utf8_and_supports_tmux_and_large_selections_are_rejected() {
        assert_eq!(clipboard_sequence("hello", false), "\x1b]52;c;aGVsbG8=\x07");
        assert_eq!(clipboard_sequence("ž", false), "\x1b]52;c;xb4=\x07");
        assert_eq!(
            clipboard_sequence("hello", true),
            "\x1bPtmux;\x1b\x1b]52;c;aGVsbG8=\x07\x1b\\"
        );
        let screen = Buffer::with_lines(vec!["x".repeat(1000); 66]);
        let mut selection = TextSelection::default();
        selection.capture(&screen, screen.area);
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 999, 65));
        assert!(selection.copy().is_err());
        assert!(copy_text(&"x".repeat(65537)).is_err());
    }

    #[test]
    fn painting_preserves_other_panes_and_keeps_the_selected_snapshot() {
        use ratatui::{Terminal, backend::TestBackend, style::Modifier};
        let screen = Buffer::with_lines(["..........", "..hello...", ".........."]);
        let area = Rect::new(2, 1, 5, 1);
        let mut selection = TextSelection::default();
        selection.capture(&screen, area);
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 1));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 6, 1));
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new("new output\nnew output\nnew output"),
                    frame.area(),
                );
                selection.capture(frame.buffer_mut(), area);
                selection.paint(frame, Style::new().add_modifier(Modifier::REVERSED));
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(selection.text(), "hello");
        assert_eq!(buffer[(0, 1)].symbol(), "n");
        assert_eq!(buffer[(2, 1)].symbol(), "h");
        assert!(buffer[(2, 1)].modifier.contains(Modifier::REVERSED));
        assert!(!buffer[(0, 1)].modifier.contains(Modifier::REVERSED));
        selection.capture(&screen, Rect::default());
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 1));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 6, 1));
        assert!(!selection.active());
    }
    #[test]
    fn selecting_the_second_cell_of_a_wide_character_highlights_the_whole_character() {
        use ratatui::{Terminal, backend::TestBackend, style::Modifier};
        let screen = Buffer::with_lines(["a界b", "...."]);
        let area = Rect::new(0, 0, 4, 1);
        let mut selection = TextSelection::default();
        selection.capture(&screen, area);
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 0));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 3, 0));
        let mut terminal = Terminal::new(TestBackend::new(4, 2)).unwrap();
        terminal
            .draw(|frame| {
                selection.paint(frame, Style::new().add_modifier(Modifier::REVERSED));
            })
            .unwrap();
        assert_eq!(selection.text(), "界b");
        assert!(
            !terminal.backend().buffer()[(0, 0)]
                .modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            terminal.backend().buffer()[(1, 0)]
                .modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            terminal.backend().buffer()[(3, 0)]
                .modifier
                .contains(Modifier::REVERSED)
        );
    }
}
