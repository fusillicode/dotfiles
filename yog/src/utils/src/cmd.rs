use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

/// Extension trait for [`Command`] to execute and handle errors.
pub trait CmdExt {
    /// Executes the command and returns the output.
    ///
    /// # Errors
    ///
    /// Returns a [`CmdError`] if the command fails to execute or if there's an I/O error.
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
                output: Box::new(output),
            });
        }
        Ok(output)
    }
}

/// Error type for command execution failures.
#[derive(thiserror::Error, Debug)]
pub enum CmdError {
    /// I/O error occurred during command execution.
    #[error("io error {source} - {cmd_details}")]
    Io {
        /// Details about the command that failed.
        cmd_details: CmdDetails,
        /// The underlying I/O error.
        #[backtrace]
        source: std::io::Error,
    },
    /// Command executed but returned a non-zero exit status.
    #[error("stderr {output:#?} - {cmd_details}")]
    Stderr {
        /// Details about the command that failed.
        cmd_details: CmdDetails,
        /// The command output containing error information.
        output: Box<Output>,
    },
    /// UTF-8 conversion error when processing command output.
    #[error("utf8 error {source} - {cmd_details}")]
    Utf8 {
        /// Details about the command that failed.
        cmd_details: CmdDetails,
        /// The underlying UTF-8 error.
        #[backtrace]
        source: core::str::Utf8Error,
    },
}

/// Details about a command execution, used for error reporting and debugging.
#[derive(Debug)]
pub struct CmdDetails {
    /// The arguments passed to the command.
    args: Vec<String>,
    /// The current working directory for the command, if specified.
    cur_dir: Option<PathBuf>,
    /// The name of the command being executed.
    name: String,
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

/// Formats [`CmdDetails`] for display, showing command name, arguments, and working directory.
impl core::fmt::Display for CmdDetails {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "cmd {} - args {:#?} - dir {:#?}", self.name, self.args, self.cur_dir)
    }
}

/// Creates a [`Command`] for the given program with silenced stdout and stderr in release mode.
pub fn silent_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    if !cfg!(debug_assertions) {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd
}
