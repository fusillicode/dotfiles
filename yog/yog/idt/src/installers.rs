use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use color_eyre::owo_colors::OwoColorize as _;
use ytil_cmd::Cmd;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

pub mod bash_language_server;
pub mod commitlint;
pub mod deno;
pub mod docker_langserver;
pub mod eslint_d;
pub mod graphql_lsp;
pub mod hadolint;
pub mod harper_ls;
pub mod helm_ls;
pub mod lua_ls;
pub mod marksman;
pub mod nvim;
pub mod prettierd;
pub mod quicktype;
pub mod ruff_lsp;
pub mod rust_analyzer;
pub mod shellcheck;
pub mod sql_language_server;
pub mod sqruff;
pub mod taplo;
pub mod terraform_ls;
pub mod typescript_language_server;
pub mod typos_lsp;
pub mod vscode_langservers;
pub mod yaml_language_server;

/// Trait for installing development tools.
pub trait Installer: Sync + Send {
    /// Returns the binary name.
    fn bin_name(&self) -> &'static str;

    /// Installs the tool.
    fn install(&self) -> color_eyre::Result<()>;

    /// Checks if the tool is installed.
    fn check(&self) -> Option<color_eyre::Result<String>> {
        let check_args = self.check_args()?;
        let mut cmd = Command::new(self.bin_name());
        cmd.args(check_args);

        #[allow(clippy::result_large_err)]
        let check_res = cmd
            .exec()
            .and_then(|output| {
                std::str::from_utf8(&output.stdout)
                    .map(ToOwned::to_owned)
                    .map_err(|err| CmdError::Utf8 {
                        cmd: Cmd::from(&cmd),
                        source: err,
                    })
            })
            .map_err(From::from);

        Some(check_res)
    }

    /// Execute install + optional check with timing output.
    ///
    /// # Errors
    /// - Install or check phase fails.
    fn run(&self) -> color_eyre::Result<()> {
        let start = Instant::now();

        // Install phase
        self.install().inspect_err(|err| {
            eprintln!(
                "{} error installing error=\n{}",
                self.bin_name().red().bold(),
                format!("{err:#?}").red()
            );
        })?;

        let past_install = Instant::now();

        // Check phase (optional)
        let mut check_duration = None;
        let check_start = Instant::now();
        let check_res = self.check();
        if check_res.is_some() {
            check_duration = Some(check_start.elapsed());
        }
        match check_res {
            Some(Ok(check_output)) => {
                println!(
                    "{} {} check_output=\n{}",
                    self.bin_name().green().bold(),
                    format_timing(start, past_install, check_duration),
                    check_output.trim_matches(|c| c == '\n' || c == '\r')
                );
            }
            Some(Err(err)) => {
                eprintln!(
                    "{} error checking {} error=\n{}",
                    self.bin_name().red(),
                    format_timing(start, past_install, check_duration),
                    format!("{err:#?}").red()
                );
                return Err(err);
            }
            None => {
                println!(
                    "{} {}",
                    self.bin_name().yellow().bold(),
                    format_timing(start, past_install, check_duration),
                );
            }
        }

        Ok(())
    }

    /// Returns arguments for version check.
    fn check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }
}

pub trait SystemDependent {
    fn target_arch_and_os(&self) -> (&str, &str);
}

/// Common install pattern for npm-based tools: download via npm, symlink the binary, and make it executable.
///
/// # Errors
/// - npm download, symlink creation, or chmod fails.
pub fn install_npm_tool(
    dev_tools_dir: &std::path::Path,
    bin_dir: &std::path::Path,
    bin_name: &str,
    npm_name: &str,
    packages: &[&str],
) -> color_eyre::Result<()> {
    let target_dir = crate::downloaders::npm::run(dev_tools_dir, npm_name, packages)?;
    let target = target_dir.join(bin_name);
    ytil_sys::file::ln_sf(&target, &bin_dir.join(bin_name))?;
    ytil_sys::file::chmod_x(target)?;
    Ok(())
}

/// Format phase timing summary line.
fn format_timing(start: Instant, past_install: Instant, check: Option<Duration>) -> String {
    format!(
        "install_time={:?} check_time={:?} total_time={:?}",
        past_install.duration_since(start),
        check,
        start.elapsed()
    )
}
