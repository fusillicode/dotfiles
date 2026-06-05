use std::io::IsTerminal;

use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;

use super::TAB_BAR_COLS;

pub struct TerminalGuard {
    entered_render_screen: bool,
    raw_mode_enabled: bool,
}

impl TerminalGuard {
    pub fn enable_if_terminal() -> rootcause::Result<Self> {
        let raw_mode_enabled = std::io::stdin().is_terminal();
        if raw_mode_enabled {
            crossterm::terminal::enable_raw_mode().context("failed to enable muxr client raw mode")?;
        }
        let entered_render_screen = std::io::stdout().is_terminal();
        if entered_render_screen {
            let mut stdout = std::io::stdout();
            if let Err(error) = crate::render::enter_terminal(&mut stdout) {
                if raw_mode_enabled {
                    drop(crossterm::terminal::disable_raw_mode());
                }
                return Err(error).context("failed to enter muxr client terminal screen")?;
            }
        }

        Ok(Self {
            entered_render_screen,
            raw_mode_enabled,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.entered_render_screen {
            let mut stdout = std::io::stdout();
            drop(crate::render::restore_terminal(&mut stdout));
        }
        if self.raw_mode_enabled {
            drop(crossterm::terminal::disable_raw_mode());
        }
    }
}

pub fn current_terminal_size() -> rootcause::Result<TerminalSize> {
    match crossterm::terminal::size() {
        Ok((cols, rows)) => TerminalSize::new(cols, rows),
        Err(error) => {
            // Headless callers cannot be queried by crossterm; explicit COLUMNS/LINES is the only fallback, so
            // missing terminal size still fails instead of silently guessing.
            if let Some(size) = self::terminal_size_from_env()? {
                return Ok(size);
            }
            Err(error).context("failed to read muxr terminal size")?
        }
    }
}

pub fn pane_size_for_terminal(size: &TerminalSize) -> rootcause::Result<TerminalSize> {
    let cols = size.cols().saturating_sub(TAB_BAR_COLS).max(1);
    TerminalSize::new(cols, size.rows())
}

fn terminal_size_from_env() -> rootcause::Result<Option<TerminalSize>> {
    let (Some(cols), Some(rows)) = (std::env::var("COLUMNS").ok(), std::env::var("LINES").ok()) else {
        return Ok(None);
    };
    TerminalSize::new(
        cols.parse::<u16>()
            .context("failed to parse COLUMNS terminal size fallback")?,
        rows.parse::<u16>()
            .context("failed to parse LINES terminal size fallback")?,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_size_for_terminal_when_tab_bar_has_room_reserves_sidebar_columns() -> rootcause::Result<()> {
        pretty_assertions::assert_eq!(
            pane_size_for_terminal(&TerminalSize::new(80, 24)?)?,
            TerminalSize::new(80_u16.saturating_sub(TAB_BAR_COLS), 24)?,
        );
        pretty_assertions::assert_eq!(
            pane_size_for_terminal(&TerminalSize::new(80, 1)?)?,
            TerminalSize::new(80_u16.saturating_sub(TAB_BAR_COLS), 1)?,
        );
        Ok(())
    }
}
