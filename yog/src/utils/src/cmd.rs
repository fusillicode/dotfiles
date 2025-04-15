use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

pub fn silent_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    if !cfg!(debug_assertions) {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd
}

pub trait CmdExt {
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

#[derive(thiserror::Error, Debug)]
pub enum CmdError {
    #[error("io error {source} - cmd {cmd_details}")]
    Io {
        cmd_details: CmdDetails,
        #[backtrace]
        source: std::io::Error,
    },
    #[error("stderr {output:?} - cmd {cmd_details}")]
    Stderr {
        cmd_details: CmdDetails,
        output: Box<Output>,
    },
}

#[derive(Debug)]
pub struct CmdDetails {
    name: String,
    args: Vec<String>,
    cur_dir: Option<PathBuf>,
}

impl From<&Command> for CmdDetails {
    fn from(value: &Command) -> Self {
        Self {
            name: value.get_program().to_string_lossy().to_string(),
            args: value
                .get_args()
                .map(|x| x.to_string_lossy().to_string())
                .collect(),
            cur_dir: value.get_current_dir().map(|x| x.to_path_buf()),
        }
    }
}

impl std::fmt::Display for CmdDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cmd {} - args {:?} - dir {:?}",
            self.name, self.args, self.cur_dir
        )
    }
}
