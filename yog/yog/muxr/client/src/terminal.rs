use std::io::IsTerminal;
use std::io::Write;

use crossterm::Command;
use crossterm::QueueableCommand;
use crossterm::cursor::Hide;
use crossterm::cursor::Show;
use crossterm::style::Attribute;
use crossterm::style::ResetColor;
use crossterm::style::SetAttribute;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;

const BRACKETED_PASTE_DISABLE: &[u8] = b"\x1b[?2004l";
const BRACKETED_PASTE_ENABLE: &[u8] = b"\x1b[?2004h";
const KITTY_KEYBOARD_PROTOCOL_DISABLE: &[u8] = b"\x1b[<1u";
const KITTY_KEYBOARD_PROTOCOL_ENABLE: &[u8] = b"\x1b[>1u";
const MOUSE_BUTTON_CAPTURE_DISABLE: &[u8] = b"\x1b[?1000l";
const MOUSE_BUTTON_CAPTURE_ENABLE: &[u8] = b"\x1b[?1000h";
const MOUSE_BUTTON_EVENT_CAPTURE_DISABLE: &[u8] = b"\x1b[?1002l";
const MOUSE_BUTTON_EVENT_CAPTURE_ENABLE: &[u8] = b"\x1b[?1002h";
const MOUSE_ANY_EVENT_CAPTURE_DISABLE: &[u8] = b"\x1b[?1003l";
const MOUSE_ANY_EVENT_CAPTURE_ENABLE: &[u8] = b"\x1b[?1003h";
const MOUSE_SGR_DISABLE: &[u8] = b"\x1b[?1006l";
const MOUSE_SGR_ENABLE: &[u8] = b"\x1b[?1006h";
const OSC8_CLOSE: &[u8] = b"\x1b]8;;\x1b\\";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SynchronizedOutput {
    Csi,
    Dcs,
}

impl SynchronizedOutput {
    #[must_use]
    pub fn for_term(term: Option<&str>) -> Self {
        match term {
            Some("alacritty") => Self::Dcs,
            Some(_) | None => Self::Csi,
        }
    }

    #[must_use]
    const fn start_sequence(self) -> &'static [u8] {
        match self {
            Self::Csi => b"\x1b[?2026h",
            Self::Dcs => b"\x1bP=1s\x1b\\",
        }
    }

    #[must_use]
    const fn end_sequence(self) -> &'static [u8] {
        match self {
            Self::Csi => b"\x1b[?2026l",
            Self::Dcs => b"\x1bP=2s\x1b\\",
        }
    }
}

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
            if let Err(error) = enter_terminal(&mut stdout) {
                // Enter can fail after partial mode writes, so restore before returning without a guard.
                drop(restore_terminal(&mut stdout));
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
            drop(restore_terminal(&mut stdout));
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

pub fn pane_size_for_terminal(tab_bar_width: u16, size: &TerminalSize) -> rootcause::Result<TerminalSize> {
    let cols = size.cols().saturating_sub(tab_bar_width).max(1);
    TerminalSize::new(cols, size.rows())
}

pub fn queue_synchronized_update_start(stdout: &mut impl Write, mode: SynchronizedOutput) -> rootcause::Result<()> {
    stdout
        .write_all(mode.start_sequence())
        .context("failed to write muxr synchronized render start")?;
    Ok(())
}

pub fn queue_synchronized_update_end(stdout: &mut impl Write, mode: SynchronizedOutput) -> rootcause::Result<()> {
    stdout
        .write_all(mode.end_sequence())
        .context("failed to write muxr synchronized render end")?;
    Ok(())
}

/// Enable or disable outer-terminal any-motion mouse capture.
///
/// Pane applications request this mode dynamically. Button-event capture remains enabled, so disabling any-motion
/// returns the client to the lower-volume mouse mode.
///
/// # Errors
/// - The terminal mode sequence cannot be written or flushed.
pub fn set_mouse_any_motion_capture(stdout: &mut impl Write, enabled: bool) -> rootcause::Result<()> {
    if enabled {
        queue_bytes(stdout, MOUSE_ANY_EVENT_CAPTURE_ENABLE)?;
    } else {
        // Some terminals treat mode churn around any-motion capture as a broader mouse-reporting reset. Reassert the
        // button modes muxr owns so pane selection and wheel routing keep working after an app leaves any-motion mode.
        queue_bytes(stdout, MOUSE_ANY_EVENT_CAPTURE_DISABLE)?;
        queue_bytes(stdout, MOUSE_BUTTON_CAPTURE_ENABLE)?;
        queue_bytes(stdout, MOUSE_BUTTON_EVENT_CAPTURE_ENABLE)?;
        queue_bytes(stdout, MOUSE_SGR_ENABLE)?;
    }
    stdout
        .flush()
        .context("failed to flush muxr any-motion mouse capture")?;
    Ok(())
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

fn enter_terminal(stdout: &mut impl Write) -> rootcause::Result<()> {
    queue_cmd(stdout, EnterAlternateScreen)?;
    queue_bytes(stdout, BRACKETED_PASTE_ENABLE)?;
    queue_bytes(stdout, KITTY_KEYBOARD_PROTOCOL_ENABLE)?;
    // Clear stale any-motion capture; the renderer re-enables it only when a pane requests that mode.
    queue_bytes(stdout, MOUSE_ANY_EVENT_CAPTURE_DISABLE)?;
    queue_bytes(stdout, MOUSE_BUTTON_CAPTURE_ENABLE)?;
    queue_bytes(stdout, MOUSE_BUTTON_EVENT_CAPTURE_ENABLE)?;
    queue_bytes(stdout, MOUSE_SGR_ENABLE)?;
    queue_cmd(stdout, Clear(ClearType::All))?;
    queue_cmd(stdout, Hide)?;
    stdout.flush().context("failed to flush muxr terminal enter")?;
    Ok(())
}

fn restore_terminal(stdout: &mut impl Write) -> rootcause::Result<()> {
    queue_bytes(stdout, OSC8_CLOSE)?;
    queue_bytes(stdout, KITTY_KEYBOARD_PROTOCOL_DISABLE)?;
    queue_bytes(stdout, MOUSE_SGR_DISABLE)?;
    queue_bytes(stdout, MOUSE_ANY_EVENT_CAPTURE_DISABLE)?;
    queue_bytes(stdout, MOUSE_BUTTON_EVENT_CAPTURE_DISABLE)?;
    queue_bytes(stdout, MOUSE_BUTTON_CAPTURE_DISABLE)?;
    queue_bytes(stdout, BRACKETED_PASTE_DISABLE)?;
    queue_cmd(stdout, LeaveAlternateScreen)?;
    reset_style(stdout)?;
    queue_cmd(stdout, Show)?;
    stdout.flush().context("failed to flush muxr terminal restore")?;
    Ok(())
}

fn reset_style(stdout: &mut impl Write) -> rootcause::Result<()> {
    queue_cmd(stdout, ResetColor)?;
    queue_cmd(stdout, SetAttribute(Attribute::Reset))
}

fn queue_cmd<W, C>(stdout: &mut W, cmd: C) -> rootcause::Result<()>
where
    W: Write,
    C: Command,
{
    Ok(stdout
        .queue(cmd)
        .map(|_| ())
        .context("failed to write muxr terminal mode command")?)
}

fn queue_bytes(stdout: &mut impl Write, bytes: &[u8]) -> rootcause::Result<()> {
    stdout
        .write_all(bytes)
        .context("failed to write muxr terminal mode sequence")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use rootcause::prelude::ResultExt;

    use super::*;

    #[test]
    fn test_pane_size_for_terminal_when_tab_bar_has_room_reserves_sidebar_columns() -> rootcause::Result<()> {
        let tab_bar_width = MuxrConfig::default().tab_bar.width;

        pretty_assertions::assert_eq!(
            pane_size_for_terminal(tab_bar_width, &TerminalSize::new(80, 24)?)?,
            TerminalSize::new(80_u16.saturating_sub(tab_bar_width), 24)?,
        );
        pretty_assertions::assert_eq!(
            pane_size_for_terminal(tab_bar_width, &TerminalSize::new(80, 1)?)?,
            TerminalSize::new(80_u16.saturating_sub(tab_bar_width), 1)?,
        );
        Ok(())
    }

    #[test]
    fn test_enter_terminal_writes_alternate_screen_and_clear() -> rootcause::Result<()> {
        let mut output = Vec::new();

        enter_terminal(&mut output)?;

        let rendered = String::from_utf8(output).context("muxr terminal test output was not utf8")?;
        assert2::assert!(rendered.contains("\x1b[?1049h"));
        assert2::assert!(rendered.contains("\x1b[?2004h"));
        assert2::assert!(rendered.contains("\x1b[>1u"));
        assert2::assert!(rendered.contains("\x1b[?1003l"));
        assert2::assert!(rendered.contains("\x1b[?1000h"));
        assert2::assert!(rendered.contains("\x1b[?1002h"));
        assert2::assert!(!rendered.contains("\x1b[?1003h"));
        assert2::assert!(rendered.contains("\x1b[?1006h"));
        assert2::assert!(rendered.contains("\x1b[2J"));
        assert2::assert!(rendered.contains("\x1b[?25l"));
        Ok(())
    }

    #[test]
    fn test_set_mouse_any_motion_capture_when_enabled_writes_any_motion_sequence() -> rootcause::Result<()> {
        let mut output = CountingWriter::default();

        set_mouse_any_motion_capture(&mut output, true)?;

        pretty_assertions::assert_eq!(output.rendered_string()?, "\x1b[?1003h");
        pretty_assertions::assert_eq!(output.flushes, 1);
        Ok(())
    }

    #[test]
    fn test_set_mouse_any_motion_capture_when_disabled_reasserts_button_capture() -> rootcause::Result<()> {
        let mut output = CountingWriter::default();

        set_mouse_any_motion_capture(&mut output, false)?;

        pretty_assertions::assert_eq!(
            output.rendered_string()?,
            "\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h",
        );
        pretty_assertions::assert_eq!(output.flushes, 1);
        Ok(())
    }

    #[rstest::rstest]
    #[case::alacritty(Some("alacritty"), SynchronizedOutput::Dcs)]
    #[case::xterm(Some("xterm-256color"), SynchronizedOutput::Csi)]
    #[case::unknown(None, SynchronizedOutput::Csi)]
    fn test_synchronized_output_for_term_when_term_is_known_returns_expected_mode(
        #[case] term: Option<&str>,
        #[case] expected: SynchronizedOutput,
    ) {
        pretty_assertions::assert_eq!(SynchronizedOutput::for_term(term), expected);
    }

    #[rstest::rstest]
    #[case::csi(SynchronizedOutput::Csi, "\x1b[?2026h", "\x1b[?2026l")]
    #[case::dcs(SynchronizedOutput::Dcs, "\x1bP=1s\x1b\\", "\x1bP=2s\x1b\\")]
    fn test_synchronized_update_queue_when_mode_is_selected_writes_expected_sequences(
        #[case] mode: SynchronizedOutput,
        #[case] start: &str,
        #[case] end: &str,
    ) -> rootcause::Result<()> {
        let mut output = Vec::new();

        queue_synchronized_update_start(&mut output, mode)?;
        queue_synchronized_update_end(&mut output, mode)?;

        let rendered = String::from_utf8(output).context("muxr terminal test output was not utf8")?;
        pretty_assertions::assert_eq!(rendered, format!("{start}{end}"));
        Ok(())
    }

    #[test]
    fn test_restore_terminal_writes_alternate_screen_exit_cursor_and_style_reset() -> rootcause::Result<()> {
        let mut output = Vec::new();

        restore_terminal(&mut output)?;

        let rendered = String::from_utf8(output).context("muxr terminal test output was not utf8")?;
        assert2::assert!(rendered.contains("\x1b[<1u"));
        assert2::assert!(rendered.contains("\x1b[?1006l"));
        assert2::assert!(rendered.contains("\x1b[?1003l"));
        assert2::assert!(rendered.contains("\x1b[?1002l"));
        assert2::assert!(rendered.contains("\x1b[?1000l"));
        assert2::assert!(rendered.contains("\x1b[?2004l"));
        assert2::assert!(rendered.contains("\x1b[?1049l"));
        assert2::assert!(rendered.contains("\x1b[?25h"));
        assert2::assert!(rendered.contains("\x1b[0m"));
        assert2::assert!(rendered.starts_with("\x1b]8;;\x1b\\"));
        Ok(())
    }

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl CountingWriter {
        fn rendered_string(&self) -> rootcause::Result<String> {
            Ok(String::from_utf8(self.bytes.clone()).context("muxr terminal test output was not utf8")?)
        }
    }

    impl std::io::Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes = self.flushes.saturating_add(1);
            Ok(())
        }
    }
}
