//! `cargo run -p rustline --example demo`
//! Resize the terminal to watch optional segments drop out. `p` toggles plain
//! separators, `m` cycles the mode, `q` quits.
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode};
use ratatui::{layout::Rect, style::Color};
use rustline::{Palette, Rustline, Segment, Separators};

fn main() -> io::Result<()> {
    let base = Color::Rgb(30, 30, 46);
    let text = Color::Rgb(205, 214, 244);
    let modes = [
        ("NORMAL", Color::Rgb(137, 180, 250)),
        ("INSERT", Color::Rgb(166, 227, 161)),
        ("VISUAL", Color::Rgb(245, 194, 231)),
    ];
    let (mut mode, mut plain) = (0, false);
    let mut terminal = ratatui::init();
    let result = loop {
        terminal.draw(|f| {
            let area = f.area();
            let line = Rustline::new(
                Palette { base, text },
                if plain {
                    Separators::Plain
                } else {
                    Separators::Powerline
                },
            )
            .left(Segment::new(modes[mode].0, base, modes[mode].1).required())
            .left(
                Segment::new("my-long-project-name", base, Color::Rgb(245, 194, 231))
                    .max_width(24)
                    .min_width(3),
            )
            .left(Segment::new("main", text, Color::Rgb(49, 50, 68)).min_bar_width(60))
            .left(Segment::new("rust 1.99", text, Color::Rgb(49, 50, 68)).min_bar_width(80))
            .right(Segment::new("utf-8", base, Color::Rgb(137, 180, 250)).min_bar_width(70))
            .right(
                Segment::new("Process running", base, Color::Rgb(137, 180, 250))
                    .required()
                    .bold(false),
            )
            .center("p plain · m mode · q quit · resize me", text);
            line.draw(
                f,
                Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
            );
        })?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => break Ok(()),
                KeyCode::Char('p') => plain = !plain,
                KeyCode::Char('m') => mode = (mode + 1) % modes.len(),
                _ => {}
            }
        }
    };
    ratatui::restore();
    result
}
