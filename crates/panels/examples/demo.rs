//! Resize the terminal: two panels switch between side by side and stacked.
//! `1` / `2` focus a panel, `q` quits.
use panels::{Palette, Panels, block, index_for_key};
use ratatui::{style::Color, widgets::{Paragraph, Widget}};

fn main() -> std::io::Result<()> {
    use crossterm::event::{Event, KeyCode, read};
    let palette = Palette { focused: Color::Blue, idle: Color::DarkGray };
    let spec = Panels::new([6, 6]).header(1);
    let mut focus = 0;
    let mut terminal = ratatui::init();
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let layout = spec.layout(area.width);
            let mut canvas = layout.buffer();
            Paragraph::new(" demo").render(ratatui::layout::Rect::new(0, 0, area.width, 1), &mut canvas);
            for (i, (rect, name)) in layout.rects.iter().zip(["Current project", "Actions"]).enumerate() {
                block(i, name, i == focus, palette).render(*rect, &mut canvas);
            }
            let row = layout.rects[focus].y + 1;
            let offset = layout.scroll_offset(row, area.height);
            layout.blit(&canvas, frame.buffer_mut(), area, offset);
        })?;
        if let Event::Key(key) = read()? {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char(c) => focus = index_for_key(c, 2).unwrap_or(focus),
                _ => {}
            }
        }
    }
    ratatui::restore();
    Ok(())
}
