use std::{
  io::{self, Stdout, Write},
  time::{Duration, Instant},
};

use crossterm::{cursor, event::Event, execute, queue, terminal};

use crate::{CellFlags, frame::Frame};

/// The core trait for defining terminal animations.
///
/// Implement this trait to create an animation that can be driven by an [`Animator`].
/// The lifecycle is: [`initial_frame`](Animation::initial_frame) → [`init`](Animation::init) →
/// repeated [`update`](Animation::update) calls until [`is_done`](Animation::is_done) returns true.
///
/// Terminal resize and input events are forwarded automatically by the `Animator`.
pub trait Animation {
  /// Initialize the animation. Internally calls [`init_with`](Animation::init_with) with the frame from [`initial_frame`](Animation::initial_frame).
	/// Override [`initial_frame`](Animation::initial_frame) to customize the initial frame content.
  fn init(&mut self) {
		self.init_with(self.initial_frame());
	}

	/// Initialize the animation with a specific frame. By default, this is called by [`init`](Animation::init) with the frame from [`initial_frame`](Animation::initial_frame), but you can override it to customize the initialization process.
	fn init_with(&mut self, initial: Frame);

  /// Produce the initial frame to pass to [`init`](Animation::init).
  /// Override this to seed the animation with custom content.
  /// Defaults to a blank frame matching the current terminal size.
  fn initial_frame(&self) -> Frame {
    Frame::from_terminal()
  }

  /// Advance the animation by one frame.
  /// Returns the frame to render.
  fn update(&mut self) -> Frame;

  /// Returns true when the animation has finished and should stop.
  fn is_done(&self) -> bool;

  /// Convenience inverse of [`is_done`](Animation::is_done).
  fn is_running(&self) -> bool {
    !self.is_done()
  }

  /// Called when the terminal is resized. Update internal dimensions here.
  fn resize(&mut self, w: usize, h: usize);

  /// Called when a terminal input event (key press, mouse, etc.) is received.
  /// Override this to make interactive animations. Resize events are handled
  /// separately via [`resize`](Animation::resize) and are not forwarded here.
  fn on_event(&mut self, _event: crossterm::event::Event) {
    log::trace!("Received event: {:?}", _event);
  }
}

struct RawModeGuard;

impl RawModeGuard {
  pub fn enter() -> io::Result<Self> {
    let mut stdout = io::stdout();
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    terminal::enable_raw_mode()?;

    Ok(Self)
  }
}

impl Drop for RawModeGuard {
  fn drop(&mut self) {
    let mut stdout = io::stdout();
    execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show).ok();
    terminal::disable_raw_mode().ok();
  }
}

/// Drives an [`Animation`] with frame-diffed terminal rendering, resize handling, and input forwarding.
///
/// The `Animator` manages the terminal lifecycle (alternate screen, raw mode, cursor visibility)
/// and provides a frame-rate-limited render loop that only redraws cells that have changed.
///
/// # Usage
/// ```no_run
/// let anim = Box::new(MyAnimation::new());
/// let mut animator = Animator::enter_with(anim)?;
/// while animator.tick()? {}
/// // terminal is restored automatically on drop
/// ```
///
/// For more control, use [`new`](Animator::new) and [`enter`](Animator::enter) separately:
/// ```no_run
/// let mut animator = Animator::new(Box::new(MyAnimation::new()));
/// // ... configure before entering the terminal
/// animator.enter()?;
/// while animator.tick()? {}
/// ```
pub struct Animator {
  animation: Box<dyn Animation>,
  last_frame: Option<Frame>,
  raw_mode_state: Option<RawModeGuard>,
  frame_rate: usize,
  last_cols: u16,
  last_rows: u16,
  out_channel: Stdout,
}

impl Animator {
  /// Create a new `Animator` without entering the alternate screen.
  /// Call [`enter`](Animator::enter) to activate the terminal, or use
  /// [`enter_with`](Animator::enter_with) for a one-step constructor.
  pub fn new(animation: Box<dyn Animation>) -> Self {
    let (last_cols, last_rows) = crossterm::terminal::size().unwrap_or((80, 24));
    Self {
      animation,
      last_frame: None,
      raw_mode_state: None,
      frame_rate: 24,
      last_cols,
      last_rows,
      out_channel: io::stdout(),
    }
  }

  /// Set the target frame rate. Defaults to 24 FPS.
  pub fn target_fps(mut self, fps: usize) -> Self {
    self.frame_rate = fps;
    self
  }

  /// Create a new `Animator` and immediately enter the alternate screen with raw mode.
  /// Equivalent to calling [`new`](Animator::new) followed by [`enter`](Animator::enter).
  pub fn enter_with(animation: Box<dyn Animation>) -> io::Result<Self> {
    let mut new = Self::new(animation);
    new.enter()?;
    Ok(new)
  }

  /// Enter the alternate screen, enable raw mode, and hide the cursor.
  /// Also initializes the animation with the frame from [`initial_frame`](Animation::initial_frame).
  /// Terminal state is restored automatically when the `Animator` is dropped.
  pub fn enter(&mut self) -> io::Result<()> {
    let guard = RawModeGuard::enter();
    self.animation.init();
    self.raw_mode_state = Some(guard?);
    Ok(())
  }

  /// Leave the alternate screen and restore terminal state.
  pub fn leave(&mut self) {
    self.raw_mode_state = None;
  }

  /// Advance the animation by one frame: poll events, update, and render.
  /// Terminal events are forwarded to the animation via [`on_event`](Animation::on_event) and resize events trigger a call to [`resize`](Animation::resize).
  ///
  /// Returns `Ok(true)` if the animation is still running, `Ok(false)` if it's done.
  /// Returns `Err` with `ErrorKind::Interrupted` on Ctrl+C.
  pub fn tick(&mut self) -> io::Result<bool> {
    let tick_start = Instant::now();

    if crossterm::event::poll(Duration::ZERO).unwrap_or(false) {
      let event = crossterm::event::read()?;
      match event {
        Event::Resize(cols, rows) => {
          if cols != self.last_cols || rows != self.last_rows {
            self.animation.resize(cols as usize, rows as usize);
            self.last_frame = None; // force full redraw
            self.last_cols = cols;
            self.last_rows = rows;
          }
        }
        Event::Key(key) => {
          if key.code == crossterm::event::KeyCode::Char('c')
            && key
              .modifiers
              .contains(crossterm::event::KeyModifiers::CONTROL)
          {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Interrupted"));
          } else {
            self.animation.on_event(Event::Key(key));
          }
        }
        _ => {
          self.animation.on_event(event);
        }
      }
    }

    let frame = self.animation.update();
    self.render(frame)?;

    let tick_duration = tick_start.elapsed().as_millis();
    let target = 1000 / self.frame_rate; // ms per frame
    let sleep_time = target.saturating_sub(tick_duration as usize);

    if sleep_time > 0 {
      log::trace!("tick completed with {} ms to spare", sleep_time);
      std::thread::sleep(Duration::from_millis(sleep_time as u64));
    }

    Ok(self.animation().is_running())
  }

  pub fn animation(&self) -> &dyn Animation {
    &*self.animation
  }

  #[allow(clippy::needless_range_loop)]
  /// Render the given frame to the terminal, diffing against the last rendered frame to minimize updates.
  fn render(&mut self, frame: Frame) -> io::Result<()> {
    let cells = frame.into_cells();
    let rows = cells.len();
    if rows == 0 {
      return Ok(());
    }
    let cols = cells[0].len();

    for row in 0..rows {
      for col in 0..cols {
        let changed = self
          .last_frame
          .as_ref()
          .is_none_or(|f| f.get_cell(row, col) != Some(&cells[row][col]));

        if changed {
          let cell = &cells[row][col];
          if cell.flags().contains(CellFlags::WIDE_CONTINUATION) {
            continue;
          }
          queue!(self.out_channel, cursor::MoveTo(col as u16, row as u16))?;
          write!(self.out_channel, "{cell}")?;
        }
      }
    }
    self.out_channel.flush()?;

    self.last_frame = Some(Frame::from_cells(cells));

    Ok(())
  }
}
