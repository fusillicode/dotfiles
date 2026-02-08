use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use owo_colors::OwoColorize as _;
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
    fn install(&self) -> rootcause::Result<()>;

    /// Runs the installed binary to verify it is functional.
    fn health_check(&self) -> Option<rootcause::Result<String>> {
        let args = self.health_check_args()?;
        let mut cmd = Command::new(self.bin_name());
        cmd.args(args);

        #[allow(clippy::result_large_err)]
        let res = cmd
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

        Some(res)
    }

    /// Execute install + optional health check with timing output.
    ///
    /// # Errors
    /// - Install or health check phase fails.
    fn run(&self) -> rootcause::Result<()> {
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

        // Health check phase (optional)
        let mut health_check_duration = None;
        let health_check_start = Instant::now();
        let health_check_res = self.health_check();
        if health_check_res.is_some() {
            health_check_duration = Some(health_check_start.elapsed());
        }

        match health_check_res {
            Some(Ok(health_check_output)) => {
                let styled_bin_name = if self.should_verify_checksum() {
                    self.bin_name().green().bold().to_string()
                } else {
                    self.bin_name().yellow().bold().to_string()
                };
                println!(
                    "{styled_bin_name} {} health_check_output=\n{}",
                    format_timing(start, past_install, health_check_duration),
                    health_check_output.trim_matches(|c| c == '\n' || c == '\r')
                );
            }
            Some(Err(err)) => {
                eprintln!(
                    "{} error in health check {} error=\n{}",
                    self.bin_name().red(),
                    format_timing(start, past_install, health_check_duration),
                    format!("{err:#?}").red()
                );
                return Err(err);
            }
            None => {
                let styled_bin_name = if self.should_verify_checksum() {
                    self.bin_name().magenta().bold().to_string()
                } else {
                    self.bin_name().blue().bold().to_string()
                };
                println!(
                    "{styled_bin_name} {}",
                    format_timing(start, past_install, health_check_duration),
                );
            }
        }

        Ok(())
    }

    /// Returns arguments for the health check (e.g. `--version`).
    fn health_check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }

    /// Whether the download is checksum-verified. Defaults to `true`.
    ///
    /// Override to return `false` for curl-based installers whose releases do not publish checksums.
    fn should_verify_checksum(&self) -> bool {
        true
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
) -> rootcause::Result<()> {
    let target_dir = crate::downloaders::npm::run(dev_tools_dir, npm_name, packages)?;
    let target = target_dir.join(bin_name);
    ytil_sys::file::ln_sf(&target, &bin_dir.join(bin_name))?;
    ytil_sys::file::chmod_x(target)?;
    Ok(())
}

/// Format phase timing summary line.
fn format_timing(start: Instant, past_install: Instant, health_check: Option<Duration>) -> String {
    format!(
        "install_time={:?} health_check_time={:?} total_time={:?}",
        past_install.duration_since(start),
        health_check,
        start.elapsed()
    )
}
