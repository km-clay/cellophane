# Cellophane

A terminal animation framework for Rust. Implement one trait, get a complete
rendering pipeline with frame diffing, resize handling, and input forwarding.

![cellophane](https://github.com/user-attachments/assets/541ccc80-a115-4893-80a1-daa7278210ed)

<sub>Animation provided by [whoa](https://github.com/km-clay/whoa)</sub>

## Features

- **One trait** - implement `Animation` and you're done
- **Frame diffing** - only changed cells are redrawn each frame
- **ANSI parsing** - `FrameBuilder` parses ansi-styled text (including 24-bit color) into a cell grid via VTE
- **Resize handling** - terminal resize events are detected and forwarded automatically
- **Input forwarding** - key and mouse events are passed to your animation for interactive use
- **Terminal lifecycle** - alternate screen, raw mode, cursor visibility, and cleanup on drop
- **Unicode support** - `Grapheme` type handles multi-codepoint characters with stack allocation via `SmallVec`

## Quick start

Add cellophane to your project:

```sh
cargo add cellophane
```

Implement the `Animation` trait and let `Animator` handle the rest:

```rust
use std::time::Duration;
use cellophane::{Animation, Animator, Frame, Cell};
use cellophane::crossterm::style::Color;

struct Rainbow {
    tick: usize,
    rows: usize,
    cols: usize,
}

impl Animation for Rainbow {
    fn init(&mut self, initial: Frame) {
        let (rows, cols) = initial.dims().unwrap_or((0, 0));
        self.rows = rows;
        self.cols = cols;
    }

    fn update(&mut self, _dt: Duration) -> Frame {
        let mut frame = Frame::with_capacity(self.cols, self.rows);
        for row in 0..self.rows {
            for col in 0..self.cols {
                let hue = ((col + row + self.tick) % 256) as u8;
                if let Some(cell) = frame.get_cell_mut(row, col) {
                    *cell = Cell::default()
                        .with_bg(Color::Rgb { r: hue, g: 100, b: 255 - hue });
                }
            }
        }
        self.tick += 1;
        frame
    }

    fn is_done(&self) -> bool { false }
    fn resize(&mut self, w: usize, h: usize) {
        self.cols = w;
        self.rows = h;
    }
}

fn main() -> std::io::Result<()> {
    let anim = Box::new(Rainbow { tick: 0, rows: 0, cols: 0 });
    let mut animator = Animator::enter_with(anim)?;
    loop {
        match animator.tick() {
            Ok(true) => continue,
            Ok(false) => break,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
```

## Core types

| Type | Description |
|------|-------------|
| `Animation` | The trait you implement to define an animation |
| `Animator` | Drives your animation with a frame-rate-limited render loop |
| `Frame` | A grid of styled `Cell`s representing one frame of output |
| `FrameBuilder` | Parses ANSI-escaped text into a `Frame` |
| `Cell` | A single terminal cell with character, colors, and attributes |
| `CellFlags` | Bitflags for text attributes (bold, italic, underline, etc.) |
| `Grapheme` | A unicode grapheme cluster backed by `SmallVec<[char; 4]>` |

## How it works

The `Animator` manages the full terminal lifecycle:

1. Enters the alternate screen, enables raw mode, hides the cursor
2. Each `tick()` polls for events, calls your `update()`, and renders the frame
3. Only cells that differ from the previous frame are written to the terminal
4. Resize events call your `resize()`, input events call your `on_event()`
5. Ctrl+C returns `Err(ErrorKind::Interrupted)` for clean shutdown
6. Terminal state is restored automatically on drop

## `ratatui` Integration

Enable with:
```sh
cargo add cellophane --features=ratatui
```

This feature flag exposes the `AnimationWidget` struct, which allows `Frame` to be usable in the `ratatui` rendering pipeline.
`AnimationWidget` implements the `Widget` trait so it can be composed with other `ratatui` widgets as you would expect.
Here's an example using the `Block` widget:
```rust
fn main() -> std::io::Result<()> {
	let mut anim = SomeAnimation::new();
	anim.init();

	ratatui::run(|terminal| {
			loop {
				terminal.draw(|f| {
					let chunks = Layout::default()
					.direction(Direction::Horizontal)
					.constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
					.split(f.area());

					let block = Block::default().title("Animation").borders(ratatui::widgets::Borders::ALL);
					let block_inner = block.inner(chunks[0]);

					// resize the animation to match block_inner
					anim.resize(block_inner.width as usize, block_inner.height as usize);
					let anim_frame = anim.update(); // get the frame

					// render the block widget, and the animation frame inside of it
					f.render_widget(block, chunks[0]);
					f.render_widget(AnimationWidget::new(&anim_frame), block_inner);
				})?;

			if event::poll(Duration::from_millis(16))? {
				if event::read()?.is_key_press() {
					break Ok(());
				}
			}
		}
	})
}
```

## Built with cellophane

- [whoa](https://github.com/km-clay/whoa) - a terminal screensaver featuring EarthBound battle backgrounds, procedural simulations, cellular automata, and more
