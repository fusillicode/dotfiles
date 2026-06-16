use std::env;
use std::path::PathBuf;

use portable_pty::CommandBuilder;
use rootcause::report;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellCmd {
    program: PathBuf,
    args: Vec<String>,
}

impl ShellCmd {
    /// Build a shell cmd for a muxr pane.
    ///
    /// # Errors
    /// - The program path is empty.
    pub fn new(program: impl Into<PathBuf>) -> rootcause::Result<Self> {
        let program = program.into();
        if program.as_os_str().is_empty() {
            return Err(report!("invalid muxr shell cmd").attach("reason=program path must not be empty"));
        }

        Ok(Self {
            program,
            args: Vec::new(),
        })
    }

    /// Build a pane startup cmd with arguments.
    ///
    /// # Errors
    /// - The program path is empty.
    pub fn with_args(
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> rootcause::Result<Self> {
        let mut cmd = Self::new(program)?;
        cmd.args = args.into_iter().map(Into::into).collect();
        Ok(cmd)
    }

    /// Build the default shell cmd from `$SHELL`, falling back to `/bin/sh`.
    ///
    /// # Errors
    /// - The selected program path is empty.
    pub fn default_from_env() -> rootcause::Result<Self> {
        let program = env::var_os("SHELL")
            .filter(|value| !value.as_os_str().is_empty())
            .map_or_else(default_shell_path, PathBuf::from);

        Self::new(program)
    }

    #[must_use]
    pub fn label(&self) -> String {
        self.program
            .file_name()
            .and_then(|name| name.to_str())
            .map_or_else(|| self.program.to_string_lossy().into_owned(), ToOwned::to_owned)
    }

    #[must_use]
    pub fn label_with_args(&self) -> String {
        let mut label = self.label();
        for arg in &self.args {
            label.push(' ');
            label.push_str(arg);
        }
        label
    }

    #[must_use]
    pub fn shell_input_line(&self) -> String {
        let mut line = self::shell_quote(&self.program.to_string_lossy());
        for arg in &self.args {
            line.push(' ');
            line.push_str(&self::shell_quote(arg));
        }
        line.push('\n');
        line
    }

    pub fn cmd_builder(&self, cwd: &str) -> rootcause::Result<CommandBuilder> {
        let mut cmd = CommandBuilder::new(self.program.as_os_str());
        cmd.cwd(self::resolved_cwd(cwd)?);
        for arg in &self.args {
            cmd.arg(arg);
        }
        Ok(cmd)
    }
}

fn default_shell_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/bin/zsh")
    } else {
        PathBuf::from("/bin/sh")
    }
}

fn resolved_cwd(cwd: &str) -> rootcause::Result<PathBuf> {
    // Pane cwd comes from restored layout or shell-title metadata; falling back to the server cwd opens panes
    // elsewhere.
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return Err(report!("invalid muxr pane cwd").attach("reason=cwd must not be empty"));
    }

    let path = if cwd == "~" {
        env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| report!("invalid muxr pane cwd").attach("reason=HOME is not set"))?
    } else if let Some(rest) = cwd.strip_prefix("~/").filter(|rest| !rest.is_empty()) {
        env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| report!("invalid muxr pane cwd").attach("reason=HOME is not set"))?
            .join(rest)
    } else {
        PathBuf::from(cwd)
    };

    if !path.is_dir() {
        return Err(report!("invalid muxr pane cwd")
            .attach("reason=path is not a directory")
            .attach(format!("cwd={cwd}"))
            .attach(format!("path={}", path.display())));
    }

    Ok(path)
}

fn shell_quote(raw: &str) -> String {
    if raw.is_empty() {
        return "''".to_owned();
    }

    if raw.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'/' | b'.' | b'_' | b'-' | b':' | b'+' | b'=' | b',' | b'@' | b'%'
            )
    }) {
        return raw.to_owned();
    }

    format!("'{}'", raw.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_shell_cmd_new_when_program_is_empty_returns_error() {
        assert2::assert!(ShellCmd::new("").is_err());
    }

    #[test]
    fn test_shell_cmd_cmd_builder_when_cwd_exists_sets_cwd() -> rootcause::Result<()> {
        let cwd = tempfile::tempdir()?;
        let cmd = ShellCmd::new("/bin/sh")?.cmd_builder(cwd.path().to_string_lossy().as_ref())?;

        pretty_assertions::assert_eq!(cmd.get_cwd().map(PathBuf::from), Some(cwd.path().to_path_buf()));
        Ok(())
    }

    #[test]
    fn test_shell_cmd_cmd_builder_when_cwd_is_missing_returns_error() -> rootcause::Result<()> {
        let cwd = tempfile::tempdir()?;
        let missing = cwd.path().join("missing");

        assert2::assert!(
            ShellCmd::new("/bin/sh")?
                .cmd_builder(missing.to_string_lossy().as_ref())
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_shell_cmd_shell_input_line_quotes_shell_words() -> rootcause::Result<()> {
        let cmd = ShellCmd::with_args("/tmp/with space/cmd", ["simple", "two words", "it's"])?;

        pretty_assertions::assert_eq!(
            cmd.shell_input_line(),
            "'/tmp/with space/cmd' simple 'two words' 'it'\\''s'\n"
        );
        Ok(())
    }
}
