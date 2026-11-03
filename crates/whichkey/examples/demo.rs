//! `cargo run -p whichkey --example demo`
//! Press Space, wait 300 ms for the popup (or type quickly). `i` opens a group,
//! Backspace goes up, Esc cancels, `q` quits.
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::Rect,
    style::Color,
    widgets::{Block, Paragraph},
};
use whichkey::{Colors, Entry, Keymap, Outcome, WhichKey};

#[derive(Clone, Debug)]
enum Action {
    Save,
    Help,
    Activity,
    Tools,
}

fn main() -> io::Result<()> {
    let mut wk = WhichKey::new(Keymap::new(vec![
        Entry::action('w', "Save file", Action::Save),
        Entry::action('?', "Help", Action::Help).also('h'),
        Entry::group(
            'i',
            "Inspect",
            vec![
                Entry::action('a', "Activity", Action::Activity),
                Entry::action('t', "Tools", Action::Tools),
            ],
        ),
    ]));
    let colors = Colors {
        base: Color::Rgb(30, 30, 46),
        border: Color::Rgb(137, 180, 250),
        title: Color::Rgb(245, 194, 231),
        key: Color::Rgb(249, 226, 175),
        group: Color::Rgb(137, 180, 250),
        text: Color::Rgb(205, 214, 244),
        subtle: Color::Rgb(108, 112, 134),
    };
    let mut last = String::from("press Space");
    let mut terminal = ratatui::init();
    let result = loop {
        terminal.draw(|f| {
            let area = f.area();
            f.render_widget(
                Paragraph::new(format!("last action: {last}\n\nSpace: leader · q: quit"))
                    .block(Block::bordered().title(" whichkey demo ")),
                area,
            );
            wk.draw(
                f,
                Rect::new(area.x, area.y, area.width, area.height),
                1,
                "Space",
                colors,
            );
        })?;
        if !event::poll(Duration::from_millis(16))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match (wk.is_open(), key.code) {
            (false, KeyCode::Char('q')) => break Ok(()),
            (false, KeyCode::Char(' ')) => wk.open(),
            (true, KeyCode::Esc) => wk.cancel(),
            (true, KeyCode::Backspace) => {
                wk.back();
            }
            (true, KeyCode::Char(ch)) => match wk.press(ch) {
                Outcome::Action(action) => last = format!("{action:?}"),
                Outcome::Unknown => last = format!("no key {ch:?}"),
                Outcome::Pending | Outcome::Cancelled => {}
            },
            _ => {}
        }
    };
    ratatui::restore();
    result
}
