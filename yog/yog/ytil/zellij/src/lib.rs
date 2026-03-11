//! Query and control `Zellij` sessions and panes via the CLI.
//!
//! Thin wrappers around `zellij` subcommands for session management, pane control, focus
//! and text injection.

use std::process::Command;

use rootcause::prelude::ResultExt;
use ytil_cmd::CmdExt;

const BIN: &str = "zellij";

/// Cardinal direction for pane operations.
#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// Returns `true` when the process is running inside a Zellij session.
pub fn is_active() -> bool {
    std::env::var_os("ZELLIJ").is_some()
}

/// Returns all running Zellij sessions as `(name, display)` pairs.
///
/// `name` is the session name with ANSI codes stripped,
/// `display` is the full ANSI-formatted line from `list-sessions`.
///
/// # Errors
/// - Invoking `zellij list-sessions` fails.
pub fn list_sessions() -> rootcause::Result<Vec<Session>> {
    let mut cmd = Command::new(BIN);
    cmd.args(["list-sessions"]);
    let output = cmd
        .output()
        .map_err(|source| ytil_cmd::CmdError::Io {
            cmd: ytil_cmd::Cmd::from(&cmd),
            source,
        })
        .attach("operation=list-sessions")?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    Ok(stdout.lines().filter(|l| !l.is_empty()).map(Session::new).collect())
}

/// A Zellij session with its plain name and ANSI-formatted display string.
#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct Session {
    /// Plain session name suitable for `kill-session`.
    pub name: String,
    /// Full ANSI-formatted line from `list-sessions`.
    pub display: String,
}

impl Session {
    /// Parses a line from `zellij list-sessions` into a [`Session`].
    ///
    /// Strips ANSI escape codes and takes the first whitespace-delimited token as the name.
    fn new(line: &str) -> Self {
        let mut plain = String::with_capacity(line.len());
        let mut in_escape = false;
        for c in line.chars() {
            match (in_escape, c) {
                (true, c) if c.is_ascii_alphabetic() => in_escape = false,
                (false, '\x1b') => in_escape = true,
                (false, c) => plain.push(c),
                _ => {}
            }
        }

        Self {
            name: plain.split_whitespace().next().unwrap_or_default().to_string(),
            display: line.to_string(),
        }
    }
}

/// Forwards arbitrary arguments to `zellij` with inherited stdio.
///
/// # Errors
/// - Invoking `zellij` fails or exits with a non-zero status.
pub fn forward(args: &[String]) -> rootcause::Result<()> {
    let mut cmd = Command::new(BIN);
    cmd.args(args);
    Ok(run_interactive(&mut cmd)?)
}

/// Prints `zellij --help` directly to the terminal, preserving ANSI colors.
///
/// # Errors
/// - Invoking `zellij --help` fails.
pub fn help() -> rootcause::Result<()> {
    let mut cmd = Command::new(BIN);
    cmd.arg("--help");
    Ok(run_interactive(&mut cmd)?)
}

/// Kills a running Zellij session by name.
///
/// # Errors
/// - Invoking `zellij kill-session` fails.
pub fn kill_session(name: &str) -> rootcause::Result<()> {
    ytil_cmd::silent_cmd(BIN)
        .args(["kill-session", name])
        .exec()
        .attach(format!("session={name}"))?;
    Ok(())
}

/// Attaches to a Zellij session by name.
///
/// When already inside Zellij, switches to the session in-place to avoid nesting.
/// When outside, spawns an interactive `zellij attach` with inherited stdio.
///
/// # Errors
/// - Invoking `zellij attach` or `zellij action switch-session` fails.
pub fn attach_session(name: &str) -> rootcause::Result<()> {
    if is_active() {
        action(&["switch-session", name]).attach(format!("session={name} mode=switch"))?;
    } else {
        let mut cmd = Command::new(BIN);
        cmd.args(["attach", name]);
        run_interactive(&mut cmd).attach(format!("session={name}"))?;
    }
    Ok(())
}

/// Deletes an exited (resurrectable) Zellij session by name.
///
/// Uses `--force` to also handle running sessions.
///
/// # Errors
/// - Invoking `zellij delete-session` fails.
pub fn delete_session(name: &str) -> rootcause::Result<()> {
    ytil_cmd::silent_cmd(BIN)
        .args(["delete-session", "--force", name])
        .exec()
        .attach(format!("session={name}"))?;
    Ok(())
}

/// Runs a [`Command`] with inherited stdio so the child process can interact with
/// the terminal directly (preserving ANSI colors, TTY detection, and interactivity).
///
/// Because stdio is inherited rather than captured, `stderr` and `stdout` in the
/// returned [`CmdError::CmdFailure`](ytil_cmd::CmdError::CmdFailure) are always
/// empty — the user already saw whatever the child printed.
fn run_interactive(cmd: &mut Command) -> Result<(), Box<ytil_cmd::CmdError>> {
    let status = cmd.status().map_err(|source| {
        Box::new(ytil_cmd::CmdError::Io {
            cmd: ytil_cmd::Cmd::from(&*cmd),
            source,
        })
    })?;
    if !status.success() {
        return Err(Box::new(ytil_cmd::CmdError::CmdFailure {
            cmd: ytil_cmd::Cmd::from(&*cmd),
            stderr: String::new(),
            stdout: String::new(),
            status,
        }));
    }
    Ok(())
}

/// Returns the number of panes in the current tab.
///
/// # Errors
/// - Invoking `zellij action list-panes` fails.
pub fn pane_count() -> rootcause::Result<usize> {
    let output = Command::new(BIN).args(["action", "list-panes"]).exec()?;
    Ok(output.stdout.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count())
}

/// Runs `zellij action <args…>`.
///
/// # Errors
/// - The `zellij` binary cannot be spawned or returns a non-zero exit status.
pub fn action(args: &[&str]) -> rootcause::Result<()> {
    let full: Vec<&str> = std::iter::once("action").chain(args.iter().copied()).collect();
    ytil_cmd::silent_cmd(BIN).args(&full).exec()?;
    Ok(())
}

/// Returns the running command of the currently focused pane by parsing `zellij action list-clients`.
///
/// Returns `None` if the command column is empty (default shell) or parsing fails.
///
/// # Errors
/// - Invoking `zellij action list-clients` fails.
pub fn focused_pane_command() -> rootcause::Result<Option<String>> {
    let output = Command::new(BIN).args(["action", "list-clients"]).exec()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let cmd = stdout
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().nth(2))
        .map(String::from);
    Ok(cmd)
}

/// Moves focus to the pane in the given direction.
///
/// # Errors
/// - The underlying `zellij action` call fails.
pub fn move_focus(direction: Direction) -> rootcause::Result<()> {
    action(&["move-focus", direction.as_str()])
}

/// Sends a raw byte (e.g. `27` for ESC) to the focused pane.
///
/// # Errors
/// - The underlying `zellij action` call fails.
pub fn write_byte(byte: u8) -> rootcause::Result<()> {
    let s = byte.to_string();
    action(&["write", &s])
}

/// Types a string into the focused pane as if the user typed it.
///
/// # Errors
/// - The underlying `zellij action` call fails.
pub fn write_chars(text: &str) -> rootcause::Result<()> {
    action(&["write-chars", text])
}

/// Opens `$EDITOR` on `path` in a new pane in the given direction.
///
/// # Errors
/// - The underlying `zellij action` call fails.
pub fn edit(path: &str, direction: Direction, line_number: Option<i64>) -> rootcause::Result<()> {
    let lnum_str;
    let dir = direction.as_str();
    let mut args = vec!["edit", "--direction", dir, path];
    if let Some(n) = line_number {
        lnum_str = n.to_string();
        args.extend(["--line-number", &lnum_str]);
    }
    action(&args)
}

/// Opens a new pane running `command` in the given direction.
///
/// # Errors
/// - The underlying `zellij action` call fails.
pub fn new_pane(direction: Direction, command: &[&str]) -> rootcause::Result<()> {
    let mut args = vec!["new-pane", "--direction", direction.as_str(), "--"];
    args.extend(command);
    action(&args)
}

/// Calls `zellij action resize increase <direction>` the given number of times.
///
/// # Errors
/// - The underlying `zellij action` call fails.
pub fn resize_increase(direction: Direction, times: u32) -> rootcause::Result<()> {
    for _ in 0..times {
        action(&["resize", "increase", direction.as_str()])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const ANSI_LINE: &str = "\x1b[32;1madept-viola\x1b[m [Created \x1b[35;1m15s\x1b[m ago]";

    #[rstest]
    #[case::plain_text(
        "my-session [Created 5m ago]",
        Session {
            name: "my-session".into(),
            display: "my-session [Created 5m ago]".into(),
        },
    )]
    #[case::ansi_codes(
        ANSI_LINE,
        Session {
            name: "adept-viola".into(),
            display: ANSI_LINE.into(),
        },
    )]
    #[case::empty_line(
        "",
        Session {
            name: String::new(),
            display: String::new(),
        },
    )]
    #[case::name_only(
        "simple",
        Session {
            name: "simple".into(),
            display: "simple".into(),
        },
    )]
    fn session_new_parses_list_sessions_line(#[case] input: &str, #[case] expected: Session) {
        pretty_assertions::assert_eq!(Session::new(input), expected);
    }
}
