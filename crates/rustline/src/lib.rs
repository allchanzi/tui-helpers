//! Rustline: a lualine-style status line for Ratatui.
//!
//! The crate knows nothing about any particular application. A caller describes segments (text,
//! colors, priority) and rustline decides what fits: required segments are
//! placed first and shortened, optional ones are added only while space is left,
//! and the center text fills whatever remains. Widths are terminal cells, never
//! bytes, so wide and combining characters are safe.
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

const POWERLINE_LEFT: &str = "\u{e0b0}";
const POWERLINE_RIGHT: &str = "\u{e0b2}";
const PLAIN: &str = "|";

/// Separator glyphs between segments; `Plain` needs no special font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separators {
    Powerline,
    Plain,
}

/// Colors of the bar itself, outside any segment.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub base: Color,
    pub text: Color,
}

/// One block of the status line. Built with a fluent API.
#[derive(Clone, Debug)]
pub struct Segment {
    text: String,
    fg: Color,
    bg: Color,
    bold: bool,
    required: bool,
    max_width: usize,
    min_width: Option<usize>,
    min_bar_width: u16,
}

impl Segment {
    pub fn new(text: impl Into<String>, fg: Color, bg: Color) -> Self {
        Self {
            text: text.into(),
            fg,
            bg,
            bold: true,
            required: false,
            max_width: usize::MAX,
            min_width: None,
            min_bar_width: 0,
        }
    }
    /// Placed before any optional segment and shortened instead of dropped.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
    pub fn bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }
    /// Longest text, in cells, this segment may show.
    pub fn max_width(mut self, cells: usize) -> Self {
        self.max_width = cells;
        self
    }
    /// Optional segments may be shortened down to this many cells; by default
    /// they are shown whole or not at all.
    pub fn min_width(mut self, cells: usize) -> Self {
        self.min_width = Some(cells);
        self
    }
    /// Hide the segment on bars narrower than this many columns.
    pub fn min_bar_width(mut self, columns: u16) -> Self {
        self.min_bar_width = columns;
        self
    }
}

/// A status line: left segments, a flexible center text, right segments.
///
/// Within each side, list required segments first; display order is list order.
pub struct Rustline {
    palette: Palette,
    separators: Separators,
    left: Vec<Segment>,
    right: Vec<Segment>,
    center: String,
    center_color: Color,
}

struct Placed {
    text: String,
    fg: Color,
    bg: Color,
    bold: bool,
}

impl Rustline {
    pub fn new(palette: Palette, separators: Separators) -> Self {
        Self {
            palette,
            separators,
            left: Vec::new(),
            right: Vec::new(),
            center: String::new(),
            center_color: palette.text,
        }
    }
    pub fn left(mut self, segment: Segment) -> Self {
        self.left.push(segment);
        self
    }
    pub fn right(mut self, segment: Segment) -> Self {
        self.right.push(segment);
        self
    }
    pub fn center(mut self, text: impl Into<String>, color: Color) -> Self {
        self.center = text.into();
        self.center_color = color;
        self
    }

    fn glyph(&self, powerline: &'static str) -> &'static str {
        match self.separators {
            Separators::Powerline => powerline,
            Separators::Plain => PLAIN,
        }
    }

    /// Decide which segments fit. Returns the placed segments in display order,
    /// the cells they use, and how many columns the right side reserved.
    fn place(&self, width: usize) -> (Vec<Placed>, usize, Vec<Placed>, usize) {
        let columns = u16::try_from(width).unwrap_or(u16::MAX);
        let visible = |s: &Segment| columns >= s.min_bar_width;
        let mut left: Vec<Option<String>> = vec![None; self.left.len()];
        let mut left_used = 0usize;
        // The first displayed segment has no leading separator; others cost one cell.
        let overhead = |index: usize| if index == 0 { 2 } else { 3 };

        for (index, segment) in self.left.iter().enumerate() {
            if !segment.required || !visible(segment) {
                continue;
            }
            let budget = width
                .saturating_sub(left_used + overhead(index))
                .min(segment.max_width);
            let text = fit(&segment.text, budget);
            left_used = (left_used + cells(&text) + overhead(index)).min(width);
            left[index] = Some(text);
        }

        // Keep one column free between the two sides.
        let mut right: Vec<Option<String>> = vec![None; self.right.len()];
        let mut right_used = 0usize;
        for pass in [true, false] {
            for (index, segment) in self.right.iter().enumerate() {
                if segment.required != pass || !visible(segment) {
                    continue;
                }
                let remaining = width.saturating_sub(left_used + 1 + right_used);
                let text = fit(
                    &segment.text,
                    remaining.saturating_sub(3).min(segment.max_width),
                );
                if text.is_empty() || (!segment.required && !whole_or_min(segment, &text)) {
                    continue;
                }
                right_used += cells(&text) + 3;
                right[index] = Some(text);
            }
        }
        let left_width = width.saturating_sub(right_used);

        for (index, segment) in self.left.iter().enumerate() {
            if segment.required || !visible(segment) {
                continue;
            }
            let available = left_width.saturating_sub(left_used);
            let floor = segment.min_width.unwrap_or_else(|| cells(&segment.text));
            if available < floor.max(1) + 4 {
                continue;
            }
            let text = fit(
                &segment.text,
                available.saturating_sub(4).min(segment.max_width),
            );
            left_used += cells(&text) + overhead(index);
            left[index] = Some(text);
        }

        let collect = |segments: &[Segment], texts: Vec<Option<String>>| {
            segments
                .iter()
                .zip(texts)
                .filter_map(|(s, text)| {
                    text.map(|text| Placed {
                        text,
                        fg: s.fg,
                        bg: s.bg,
                        bold: s.bold,
                    })
                })
                .collect::<Vec<_>>()
        };
        (
            collect(&self.left, left),
            left_used,
            collect(&self.right, right),
            right_used,
        )
    }

    /// Spans for the left and right halves plus the right half's width.
    fn spans(&self, width: usize) -> (Vec<Span<'static>>, Vec<Span<'static>>, usize) {
        let (left, left_used, right, right_width) = self.place(width);
        let left_width = width.saturating_sub(right_width);
        let style = |p: &Placed| {
            let style = Style::new().fg(p.fg).bg(p.bg);
            if p.bold {
                style.add_modifier(Modifier::BOLD)
            } else {
                style
            }
        };

        let mut spans = Vec::new();
        let mut previous: Option<Color> = None;
        for placed in &left {
            if let Some(previous) = previous {
                spans.push(Span::styled(
                    self.glyph(POWERLINE_LEFT),
                    Style::new().fg(previous).bg(placed.bg),
                ));
            }
            spans.push(Span::styled(format!(" {} ", placed.text), style(placed)));
            previous = Some(placed.bg);
        }
        let mut used = left_used;
        // Powerline needs the closing arrow; a plain bar only needs a bar when
        // center text follows it, otherwise it would touch the right half's bar.
        let center_follows = !self.center.is_empty() && left_width.saturating_sub(used + 1) > 1;
        if let Some(previous) = previous
            && used < left_width
            && (self.separators == Separators::Powerline || center_follows)
        {
            spans.push(Span::styled(
                self.glyph(POWERLINE_LEFT),
                Style::new().fg(previous).bg(self.palette.base),
            ));
            used += 1;
        }
        if left_width.saturating_sub(used) > 1 && !self.center.is_empty() {
            spans.push(Span::styled(
                format!(" {}", fit(&self.center, left_width - used - 1)),
                Style::new().fg(self.center_color).bg(self.palette.base),
            ));
        }

        let mut tail = Vec::new();
        let mut previous = self.palette.base;
        for placed in &right {
            tail.push(Span::styled(
                self.glyph(POWERLINE_RIGHT),
                Style::new().fg(placed.bg).bg(previous),
            ));
            tail.push(Span::styled(format!(" {} ", placed.text), style(placed)));
            previous = placed.bg;
        }
        (spans, tail, right_width)
    }

    pub fn draw(&self, frame: &mut Frame<'_>, area: Rect) {
        if area.is_empty() {
            return;
        }
        let background = Style::new().fg(self.palette.text).bg(self.palette.base);
        frame.render_widget(Paragraph::new("").style(background), area);
        let width = usize::from(area.width);
        let (left, right, right_width) = self.spans(width);
        let right_width = u16::try_from(right_width)
            .unwrap_or(area.width)
            .min(area.width);
        frame.render_widget(
            Paragraph::new(Line::from(left)).style(background),
            Rect::new(area.x, area.y, area.width - right_width, 1),
        );
        if right_width > 0 {
            frame.render_widget(
                Paragraph::new(Line::from(right)),
                Rect::new(area.right() - right_width, area.y, right_width, 1),
            );
        }
    }
}

fn whole_or_min(segment: &Segment, fitted: &str) -> bool {
    let floor = segment.min_width.unwrap_or_else(|| cells(&segment.text));
    cells(fitted) >= floor.min(cells(&segment.text))
}

fn cells(text: &str) -> usize {
    Span::raw(text).width()
}

/// Truncate by grapheme and terminal cells, preserving combining sequences.
pub fn fit(text: &str, width: usize) -> String {
    let span = Span::raw(text);
    let clean: String = span
        .styled_graphemes(Style::default())
        .map(|g| g.symbol)
        .collect();
    if cells(&clean) <= width {
        return clean;
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for g in span.styled_graphemes(Style::default()) {
        let w = cells(g.symbol);
        if used + w > width - 1 {
            break;
        }
        result.push_str(g.symbol);
        used += w;
    }
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    const BASE: Color = Color::Rgb(0, 0, 0);
    const TEXT: Color = Color::Rgb(255, 255, 255);
    const BLUE: Color = Color::Blue;
    const PINK: Color = Color::Magenta;

    fn line(separators: Separators) -> Rustline {
        Rustline::new(
            Palette {
                base: BASE,
                text: TEXT,
            },
            separators,
        )
        .left(Segment::new("NORMAL", BASE, BLUE).required().max_width(12))
        .left(
            Segment::new("项目e\u{301}-very-long-project-name", BASE, PINK)
                .max_width(24)
                .min_width(3),
        )
        .left(Segment::new("Codex", BASE, PINK).min_bar_width(80))
        .right(
            Segment::new("Process running", BASE, BLUE)
                .required()
                .bold(false),
        )
        .center("hint", TEXT)
    }

    fn render(line: &Rustline, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal.draw(|f| line.draw(f, f.area())).unwrap();
        let mut out = String::new();
        let mut x = 0;
        while x < width {
            let symbol = terminal.backend().buffer()[(x, 0)].symbol();
            out.push_str(symbol);
            x += cells(symbol).max(1) as u16;
        }
        out
    }

    #[test]
    fn required_segments_survive_and_optional_ones_yield_by_width() {
        let line = line(Separators::Plain);
        for width in [100, 60, 40] {
            let out = render(&line, width);
            assert!(out.starts_with(" NORMAL "), "{out}");
            assert!(out.ends_with("Process running "), "{out}");
            assert!(out.contains("项目e\u{301}"), "{out}");
            assert_eq!(out.contains("Codex"), width >= 80, "{out}");
        }
        // Very narrow bars never panic and never overlap the sides.
        for width in 0..24 {
            let _ = render(&line, width);
        }
    }

    #[test]
    fn plain_separators_avoid_powerline_glyphs_and_powerline_uses_them() {
        assert!(!render(&line(Separators::Plain), 100).contains(['\u{e0b0}', '\u{e0b2}']));
        let out = render(&line(Separators::Powerline), 100);
        assert!(
            out.contains('\u{e0b0}') && out.contains('\u{e0b2}'),
            "{out}"
        );
    }

    #[test]
    fn plain_bar_has_no_doubled_separator_when_halves_meet() {
        let line = Rustline::new(
            Palette {
                base: BASE,
                text: TEXT,
            },
            Separators::Plain,
        )
        .left(Segment::new("NORMAL", BASE, BLUE).required())
        .right(Segment::new("Process running", BASE, BLUE).required())
        .center("hint", TEXT);
        for width in 24..40 {
            let out = render(&line, width);
            assert!(
                !out.contains("||") && !out.contains("| |"),
                "{width}: {out}"
            );
        }
    }

    #[test]
    fn center_text_takes_only_leftover_space() {
        let out = render(&line(Separators::Plain), 100);
        assert!(out.contains("hint"), "{out}");
        let out = render(&line(Separators::Plain), 30);
        assert!(!out.contains("hint"), "{out}");
    }

    #[test]
    fn fit_counts_cells_and_keeps_combining_sequences() {
        assert_eq!(fit("项目e\u{301}abc", 6), "项目e\u{301}…");
        assert_eq!(fit("项目", 1), "…");
        assert_eq!(fit("项目", 0), "");
        assert_eq!(fit("short", 10), "short");
    }
}
