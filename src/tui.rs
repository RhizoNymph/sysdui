use anyhow::Result;
use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, stdout};

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;

pub fn init() -> Result<Tui> {
    terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        cursor::Hide
    )?;
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore() -> Result<()> {
    crossterm::execute!(
        stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        cursor::Show
    )?;
    terminal::disable_raw_mode()?;
    Ok(())
}

/// Suspend the TUI so we can shell out (e.g. for sudo, $EDITOR).
pub fn suspend() -> Result<()> {
    crossterm::execute!(
        stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        cursor::Show
    )?;
    terminal::disable_raw_mode()?;
    Ok(())
}

/// Resume the TUI after shelling out.
pub fn resume() -> Result<Tui> {
    init()
}
