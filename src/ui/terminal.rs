use super::*;

pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}
pub(super) fn restore() {
    // On Windows mouse capture restores the console mode saved while raw mode
    // was active. Release it first so it cannot re-enable raw input after exit.
    let _ = execute!(stdout(), DisableMouseCapture);
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        crossterm::cursor::Show,
        crossterm::style::Print("\x1b[23;0t")
    );
}
impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        enable_raw_mode()?;
        if let Err(e) = execute!(
            stdout(),
            crossterm::style::Print("\x1b[22;0t"),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture,
            SetTitle("Tuitify")
        ) {
            restore();
            return Err(e.into());
        }
        match Terminal::new(CrosstermBackend::new(stdout())) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(e) => {
                restore();
                Err(e.into())
            }
        }
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

pub fn set_title(title: &str) {
    let _ = execute!(stdout(), SetTitle(title));
}
