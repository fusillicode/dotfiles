//! Execute system commands with structured errors and optional silenced output in release builds.
//!
//! Exposes an extension trait [`CmdExt`] with an `exec` method plus a helper [`silent_cmd`] that
//! null-routes stdout/stderr outside debug mode. Errors capture the command name, args and working
//! directory for concise diagnostics.
//!
//! See [`CmdError`] for failure variants with rich context.
#![feature(error_generic_member_access)]

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Output;
use std::process::Stdio;

/// Extension trait for [`Command`] to execute and handle errors.
pub trait CmdExt {
    /// Run the command; capture stdout & stderr; return [`Output`] on success.
    ///
    /// # Returns
    /// Full [`Output`] (captured stdout & stderr) when the exit status is zero.
    ///
    /// # Errors
    /// - Spawning or waiting fails ([`CmdError::Io`]).
    /// - Non-zero exit with valid UTF-8 stderr ([`CmdError::Stderr`]).
    /// - Non-zero exit with invalid UTF-8 stderr ([`CmdError::FromUtf8`]).
    /// - Borrowed UTF-8 validation failure ([`CmdError::Utf8`]).
    fn exec(&mut self) -> color_eyre::Result<Output, CmdError>;
}

impl CmdExt for Command {
    fn exec(&mut self) -> color_eyre::Result<Output, CmdError> {
        let output = self.output().map_err(|source| CmdError::Io {
            cmd_details: CmdDetails::from(&*self),
            source,
        })?;
        if !output.status.success() {
            return Err(CmdError::Stderr {
                cmd_details: CmdDetails::from(&*self),
                output: String::from_utf8(output.stderr).map_err(|error| CmdError::FromUtf8 {
                    cmd_details: CmdDetails::from(&*self),
                    source: error,
                })?,
                status: output.status,
            });
        }
        Ok(output)
    }
}

/// Command execution errors with contextual details.
///
/// Each variant embeds [`CmdDetails`] (program, args, cwd) for terse diagnostics. `Utf8`
/// is currently not produced by [`CmdExt::exec`] but kept for potential future APIs.
#[derive(thiserror::Error, Debug)]
pub enum CmdError {
    /// Non-zero exit status; stderr captured & UTF-8 decoded.
    #[error("non-zero status output={output:?} status={status:?} {cmd_details})")]
    Stderr {
        /// Command metadata snapshot.
        cmd_details: CmdDetails,
        /// Full (untruncated) stderr.
        output: String,
        /// Failing status.
        status: ExitStatus,
    },
    /// I/O failure spawning or waiting.
    #[error("{source} {cmd_details}")]
    Io {
        /// Command metadata snapshot.
        cmd_details: CmdDetails,
        #[backtrace]
        /// Underlying OS error.
        source: std::io::Error,
    },
    /// Borrowed data UTF-8 validation failed.
    #[error("{source} {cmd_details}")]
    Utf8 {
        /// Command metadata snapshot.
        cmd_details: CmdDetails,
        #[backtrace]
        /// UTF-8 error.
        source: core::str::Utf8Error,
    },
    /// Owned stderr bytes not valid UTF-8.
    #[error("{source} {cmd_details}")]
    FromUtf8 {
        /// Command metadata snapshot.
        cmd_details: CmdDetails,
        #[backtrace]
        /// Conversion error.
        source: std::string::FromUtf8Error,
    },
}

/// Snapshot of command name, args and cwd.
///
/// Arguments/program are converted lossily from [`std::ffi::OsStr`] to [`String`] for ease of logging.
#[derive(Debug)]
pub struct CmdDetails {
    /// Ordered arguments (lossy UTF-8).
    args: Vec<String>,
    /// Working directory (if set).
    cur_dir: Option<PathBuf>,
    /// Program / executable name.
    name: String,
}

/// Formats [`CmdDetails`] for display, showing command name, arguments, and working directory.
impl core::fmt::Display for CmdDetails {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "CmdDetails=(name={:?} args={:?} cur_dir={:?})",
            self.name, self.args, self.cur_dir,
        )
    }
}

/// Converts a [`Command`] reference to [`CmdDetails`] for error reporting.
impl From<&Command> for CmdDetails {
    fn from(value: &Command) -> Self {
        Self {
            name: value.get_program().to_string_lossy().to_string(),
            args: value.get_args().map(|x| x.to_string_lossy().to_string()).collect(),
            cur_dir: value.get_current_dir().map(Path::to_path_buf),
        }
    }
}

/// Converts a mutable [`Command`] reference to [`CmdDetails`] for error reporting.
impl From<&mut Command> for CmdDetails {
    fn from(value: &mut Command) -> Self {
        Self {
            name: value.get_program().to_string_lossy().to_string(),
            args: value.get_args().map(|x| x.to_string_lossy().to_string()).collect(),
            cur_dir: value.get_current_dir().map(Path::to_path_buf),
        }
    }
}

/// Creates a [`Command`] for `program`; silences stdout/stderr in release builds.
///
/// In debug (`debug_assertions`), output is inherited for easier troubleshooting.
/// In release, both streams are redirected to [`Stdio::null()`] to keep logs quiet.
pub fn silent_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    if !cfg!(debug_assertions) {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_success_returns_output() {
        let mut cmd = Command::new("bash");
        cmd.args(["-c", "echo -n ok"]);

        assert2::let_assert!(Ok(out) = cmd.exec());
        assert!(out.status.success());
        assert_eq!("ok", String::from_utf8(out.stdout).unwrap());
        assert_eq!("", String::from_utf8(out.stderr).unwrap());
    }

    #[test]
    fn exec_captures_non_zero_status() {
        let mut cmd = Command::new("bash");
        cmd.args(["-c", "echo foo error 1>&2; exit 7"]);

        assert2::let_assert!(Err(CmdError::Stderr { status, output, .. }) = cmd.exec());
        assert_eq!(Some(7), status.code());
        assert!(output.contains("foo err"));
    }
}
