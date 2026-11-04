//! Responsive numbered panels, in the spirit of lazygit / lazydocker.
//!
//! Panels sit **side by side** when the terminal is wide and **stacked** when it
//! is narrow. Each panel has a number (`[1] Title`) so an application can map the
//! digit keys to "focus panel n". When the stacked layout is taller than the
//! screen, the content is scrolled so the focused row stays visible.
//!
//! The crate owns no input and no content:
//! 1. [`Panels::layout`] turns a width into an [`Arrangement`] (panel rects in a
//!    *virtual*, unscrolled canvas).
//! 2. The application renders any widgets into a [`Buffer`] of
//!    [`Arrangement::canvas`] size (use [`block`] for the numbered frame).
//! 3. [`Arrangement::blit`] copies the visible window to the real screen, and
//!    [`Arrangement::hit`] maps a mouse position back to a virtual position.
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    widgets::Block,
};

/// Describes how many panels there are and when to switch between layouts.
#[derive(Clone, Debug)]
pub struct Panels {
    /// Rows above the panels (title, breadcrumb, ...), common to both layouts.
    pub header: u16,
    /// Outer height of each panel, borders included.
    pub heights: Vec<u16>,
    /// Width (in columns) from which panels sit side by side.
    pub wide_from: u16,
}

impl Panels {
    pub fn new(heights: impl Into<Vec<u16>>) -> Self {
        Self {
            header: 0,
            heights: heights.into(),
            wide_from: 96,
        }
    }
    pub fn header(mut self, rows: u16) -> Self {
        self.header = rows;
        self
    }
    pub fn wide_from(mut self, columns: u16) -> Self {
        self.wide_from = columns;
        self
    }
    /// Lay the panels out for a terminal `width` columns wide.
    ///
    /// Side by side the width is divided evenly (the first panels get the
    /// remainder-free half); stacked every panel takes the full width.
    pub fn layout(&self, width: u16) -> Arrangement {
        let count = self.heights.len() as u16;
        let wide = count > 1 && width >= self.wide_from;
        let mut rects = Vec::with_capacity(self.heights.len());
        let height;
        if wide {
            let mut x = 0;
            for (i, h) in self.heights.iter().enumerate() {
                let right = (width as u32 * (i as u32 + 1) / count as u32) as u16;
                rects.push(Rect::new(x, self.header, right - x, *h));
                x = right;
            }
            height = self.header + self.heights.iter().copied().max().unwrap_or(0);
        } else {
            let mut y = self.header;
            for h in &self.heights {
                rects.push(Rect::new(0, y, width, *h));
                y += h;
            }
            height = y;
        }
        Arrangement {
            rects,
            width,
            height,
            side_by_side: wide,
        }
    }
}

/// Panel rectangles in virtual (unscrolled) coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrangement {
    pub rects: Vec<Rect>,
    pub width: u16,
    pub height: u16,
    pub side_by_side: bool,
}

impl Arrangement {
    /// The virtual canvas to render into.
    pub fn canvas(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }
    /// A buffer for the virtual canvas.
    pub fn buffer(&self) -> Buffer {
        Buffer::empty(self.canvas())
    }
    /// First visible canvas row so that `focus_row` is on screen in a viewport
    /// of `viewport_height` rows. Never scrolls past the end of the canvas.
    pub fn scroll_offset(&self, focus_row: u16, viewport_height: u16) -> u16 {
        let viewport = viewport_height.max(1);
        focus_row
            .saturating_sub(viewport - 1)
            .min(self.height.saturating_sub(viewport))
    }
    /// Copy the visible window of `canvas` (starting at row `offset`) into
    /// `target`, inside `area`. Rows beyond the canvas are left untouched.
    pub fn blit(&self, canvas: &Buffer, target: &mut Buffer, area: Rect, offset: u16) {
        for y in 0..area.height {
            let vy = y + offset;
            if vy >= canvas.area.height {
                break;
            }
            for x in 0..area.width.min(canvas.area.width) {
                target[(area.x + x, area.y + y)] = canvas[(x, vy)].clone();
            }
        }
    }
    /// Translate a screen position inside `area` to canvas coordinates.
    pub fn hit(&self, area: Rect, offset: u16, column: u16, row: u16) -> Option<Position> {
        area.contains(Position::new(column, row))
            .then(|| Position::new(column - area.x, row - area.y + offset))
    }
    /// Index of the panel containing a canvas position.
    pub fn panel_at(&self, position: Position) -> Option<usize> {
        self.rects.iter().position(|r| r.contains(position))
    }
}

/// `" [n] Title "`, with `n` counted from 1.
pub fn title(number: usize, text: &str) -> String {
    format!(" [{}] {} ", number + 1, text)
}

/// Panel index for a digit key: `'1'` is panel 0. `None` for other keys or a
/// digit beyond `count`.
pub fn index_for_key(key: char, count: usize) -> Option<usize> {
    let n = key.to_digit(10)? as usize;
    (1..=count.min(9)).contains(&n).then(|| n - 1)
}

/// Colors of a panel frame.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub focused: Color,
    pub idle: Color,
}

/// A bordered, numbered panel frame; the focused one is highlighted.
pub fn block(number: usize, text: &str, focused: bool, palette: Palette) -> Block<'static> {
    let color = if focused { palette.focused } else { palette.idle };
    let mut style = Style::new().fg(color);
    if focused {
        style = style.add_modifier(Modifier::BOLD);
    }
    Block::bordered()
        .title(title(number, text))
        .title_style(style)
        .border_style(Style::new().fg(color))
}


/// Proportional multi-column layout for dashboards: columns of weighted panels
/// that collapse into a single stacked column when the area is narrow.
///
/// Rects come back in *spec order* (column by column, top to bottom), so the
/// caller can number the panels in that order.
#[derive(Clone, Debug)]
pub struct Flow {
    columns: Vec<Vec<u16>>,
    wide_from: u16,
    focus_boost: u16,
}

impl Flow {
    /// `columns[c][r]` is the weight of the r-th panel of column c. Columns share
    /// the width evenly.
    pub fn new(columns: impl Into<Vec<Vec<u16>>>) -> Self {
        Self {
            columns: columns.into(),
            wide_from: 100,
            focus_boost: 2,
        }
    }
    /// Width from which the columns are shown side by side.
    pub fn wide_from(mut self, columns: u16) -> Self {
        self.wide_from = columns;
        self
    }
    /// In the stacked layout the focused panel's weight is multiplied by this,
    /// so it gets more room (lazygit-style accordion). `1` disables it.
    pub fn focus_boost(mut self, factor: u16) -> Self {
        self.focus_boost = factor.max(1);
        self
    }
    /// Number of panels.
    pub fn len(&self) -> usize {
        self.columns.iter().map(Vec::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// True when `area` is wide enough for side-by-side columns.
    pub fn is_wide(&self, area: Rect) -> bool {
        self.columns.len() > 1 && area.width >= self.wide_from
    }
    /// Panel rects in spec order. `focus` is the spec-order index of the focused
    /// panel (only used by the stacked layout).
    pub fn layout(&self, area: Rect, focus: Option<usize>) -> Vec<Rect> {
        if self.is_wide(area) {
            let count = self.columns.len() as u32;
            let mut rects = Vec::with_capacity(self.len());
            let mut left = area.x;
            for (c, weights) in self.columns.iter().enumerate() {
                let right = area.x + (area.width as u32 * (c as u32 + 1) / count) as u16;
                let column = Rect::new(left, area.y, right - left, area.height);
                rects.extend(split(column, weights.iter().copied()));
                left = right;
            }
            rects
        } else {
            let focus = focus.unwrap_or(usize::MAX);
            let weights = self.columns.iter().flatten().enumerate().map(|(i, w)| {
                if i == focus { w.saturating_mul(self.focus_boost) } else { *w }
            });
            split(area, weights)
        }
    }
}

/// Split `area` vertically by weights; every panel gets at least `3` rows when
/// the area allows it, the remaining rows follow the weights.
fn split(area: Rect, weights: impl Iterator<Item = u16>) -> Vec<Rect> {
    let weights: Vec<u16> = weights.map(|w| w.max(1)).collect();
    let n = weights.len() as u16;
    if n == 0 {
        return Vec::new();
    }
    const MIN: u16 = 3;
    let floor = if area.height >= MIN * n { MIN } else { area.height / n };
    let spare = area.height - floor * n;
    let total: u32 = weights.iter().map(|w| *w as u32).sum();
    let mut y = area.y;
    let mut given = 0u32;
    let mut cumulative = 0u32;
    weights
        .iter()
        .enumerate()
        .map(|(i, w)| {
            cumulative += *w as u32;
            let target = spare as u32 * cumulative / total;
            let extra = (target - given) as u16;
            given = target;
            let mut h = floor + extra;
            if i as u16 == n - 1 {
                h = area.bottom() - y;
            }
            let rect = Rect::new(area.x, y, area.width, h);
            y += h;
            rect
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two() -> Panels {
        Panels::new([6, 6]).header(2).wide_from(96)
    }

    #[test]
    fn wide_puts_panels_side_by_side_and_covers_the_width() {
        let a = two().layout(100);
        assert!(a.side_by_side);
        assert_eq!(a.rects, [Rect::new(0, 2, 50, 6), Rect::new(50, 2, 50, 6)]);
        assert_eq!(a.height, 8);
        let odd = two().layout(97);
        assert_eq!(odd.rects[0].width + odd.rects[1].width, 97);
        assert_eq!(odd.rects[1].right(), 97);
    }

    #[test]
    fn narrow_stacks_panels_at_full_width() {
        let a = two().layout(95);
        assert!(!a.side_by_side);
        assert_eq!(a.rects, [Rect::new(0, 2, 95, 6), Rect::new(0, 8, 95, 6)]);
        assert_eq!(a.height, 14);
    }

    #[test]
    fn a_single_panel_never_splits() {
        assert!(!Panels::new([5]).layout(200).side_by_side);
    }

    #[test]
    fn scrolling_keeps_the_focus_visible_and_stops_at_the_end() {
        let a = two().layout(60);
        assert_eq!(a.scroll_offset(3, 6), 0);
        assert_eq!(a.scroll_offset(10, 6), 5);
        assert_eq!(a.scroll_offset(13, 6), 8);
        assert_eq!(a.scroll_offset(13, 100), 0);
    }

    #[test]
    fn digit_keys_map_to_panels() {
        assert_eq!(index_for_key('1', 2), Some(0));
        assert_eq!(index_for_key('2', 2), Some(1));
        assert_eq!(index_for_key('3', 2), None);
        assert_eq!(index_for_key('0', 2), None);
        assert_eq!(index_for_key('x', 2), None);
        assert_eq!(title(0, "Actions"), " [1] Actions ");
    }

    #[test]
    fn blit_and_hit_round_trip_through_the_scroll_offset() {
        let a = two().layout(60);
        let mut canvas = a.buffer();
        canvas[(3, 10)].set_symbol("X");
        let area = Rect::new(0, 0, 60, 6);
        let mut screen = Buffer::empty(area);
        let offset = a.scroll_offset(10, 6);
        a.blit(&canvas, &mut screen, area, offset);
        assert_eq!(screen[(3, 10 - offset)].symbol(), "X");
        let p = a.hit(area, offset, 3, 10 - offset).unwrap();
        assert_eq!(p, Position::new(3, 10));
        assert_eq!(a.panel_at(p), Some(1));
        assert_eq!(a.hit(area, offset, 60, 0), None);
    }

    #[test]
    fn flow_splits_columns_when_wide_and_collapses_when_narrow() {
        let flow = Flow::new([vec![22, 18, 60], vec![60, 40]]).wide_from(100);
        let wide = flow.layout(Rect::new(0, 0, 120, 40), None);
        assert_eq!(wide.len(), 5);
        assert_eq!(wide[0].x, 0);
        assert_eq!(wide[3].x, 60);
        assert_eq!(wide[2].bottom(), 40);
        assert_eq!(wide[4].bottom(), 40);
        let narrow = flow.layout(Rect::new(0, 0, 80, 40), Some(3));
        assert_eq!(narrow.len(), 5);
        assert!(narrow.iter().all(|r| r.x == 0 && r.width == 80));
        assert_eq!(narrow[4].bottom(), 40);
        for pair in narrow.windows(2) {
            assert_eq!(pair[0].bottom(), pair[1].y);
        }
        // The focused panel gets more room than it would without the boost.
        let flat = Flow::new([vec![22, 18, 60], vec![60, 40]])
            .focus_boost(1)
            .layout(Rect::new(0, 0, 80, 40), Some(3));
        assert!(narrow[3].height > flat[3].height);
    }

    #[test]
    fn flow_gives_every_panel_room_on_short_screens() {
        let flow = Flow::new([vec![1, 1, 1]]);
        let rects = flow.layout(Rect::new(0, 0, 50, 9), None);
        assert!(rects.iter().all(|r| r.height == 3));
        let tiny = flow.layout(Rect::new(0, 0, 50, 2), None);
        assert_eq!(tiny.iter().map(|r| r.height).sum::<u16>(), 2);
    }
}
