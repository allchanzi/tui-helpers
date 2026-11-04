# tui-helpers

Small, independent [Ratatui](https://ratatui.rs) crates that bring the
Neovim look and feel to terminal apps. They know nothing about any particular
application; colors and actions come from the caller.

| Crate | What it is | Neovim counterpart |
| --- | --- | --- |
| [`rustline`](crates/rustline) | Responsive status line with segments, Powerline or plain separators, priorities | lualine |
| [`whichkey`](crates/whichkey) | Key tree, pending-prefix state, delayed popup, breadcrumb | which-key.nvim |
| [`panels`](crates/panels) | Numbered `[1] Title` panels, side by side when wide and stacked when narrow, scrolled to the focus | lazygit / lazydocker |

All target Ratatui 0.30 and Rust 2024. None needs a backend feature, so they
work with any Ratatui backend. Run the demos in a real terminal:

```sh
cargo run -p whichkey --example demo
cargo run -p rustline --example demo
cargo run -p panels --example demo
```

## Use them in another project

```toml
[dependencies]
# local checkout
rustline = { path = "../tui-helpers/crates/rustline" }
whichkey = { path = "../tui-helpers/crates/whichkey" }
# or from a git remote once you have pushed this repository
# rustline = { git = "https://example.invalid/you/tui-helpers" }
```

Use the same `ratatui` minor version as these crates (0.30), otherwise the
`Frame`/`Rect`/`Color` types will not match.

## rustline

```rust
use rustline::{Palette, Rustline, Segment, Separators};

Rustline::new(Palette { base, text }, Separators::Powerline)
    .left(Segment::new("NORMAL", base, blue).required().max_width(12))
    .left(Segment::new(project, base, pink).max_width(24).min_width(3))
    .left(Segment::new("main", text, surface).min_bar_width(80))
    .right(Segment::new("Process running", base, blue).required().bold(false))
    .center("hint or message", text)
    .draw(frame, area); // area is one row
```

- **Required** segments are placed first and shortened, never dropped.
- **Optional** segments appear while there is room. `min_width` lets one shrink
  to that many cells; `min_bar_width` hides it on narrow bars.
- The **center** text fills whatever is left.
- Widths are terminal cells (wide and combining characters are safe);
  `Separators::Plain` avoids Powerline glyphs for fonts without them.
- List required segments before optional ones on each side; display order is
  list order.

## whichkey

```rust
use whichkey::{Colors, Entry, Keymap, Outcome, WhichKey};

let mut wk = WhichKey::new(Keymap::new(vec![
    Entry::action('w', "Save", Action::Save),
    Entry::action('?', "Help", Action::Help).also('h'),
    Entry::group('g', "Git", vec![Entry::action('c', "Commit", Action::Commit)]),
]));

// on your leader key:
wk.open();
// on each following character:
match wk.press(ch) {
    Outcome::Action(a) => run(a),
    Outcome::Pending => {}                // a group opened
    Outcome::Unknown | Outcome::Cancelled => {}
}
// Backspace -> wk.back(), Esc -> wk.cancel()
// every frame:
wk.draw(frame, frame.area(), 2 /* rows kept free for your footer */, "Space", colors);
```

The popup appears after 300 ms (`WhichKey::delay` changes it) at the top level and
immediately inside a group, so fast typists never see it. `wk.summary()` returns
the current level as one line for a status hint, so the popup, hint and help text
all come from the same tree. `Keymap` alone is the pure tree if you manage the
pending state yourself.

## Status

Extracted from [fleetui](../fleetui), where both modules were written and tested.
Used by fleetui, [pde](../pde) (`pde_tui`), [rtui](../rtui) and [pantsui](../pantsui) through path
dependencies, all with a `Space` leader tree and the Neovim (catppuccin-mocha) palette.
Verified here: unit tests, clippy and the demos in a tmux session (popup delay,
groups, narrowing bars). Not verified: a Powerline font's glyph coverage on your
terminal, other terminals, and accessibility tooling. No license is set yet.
