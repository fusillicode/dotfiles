use std::process::Command;

use color_eyre::owo_colors::OwoColorize as _;
use ytil_cmd::Cmd;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt;

pub mod bash_language_server;
pub mod commitlint;
pub mod deno;
pub mod docker_langserver;
pub mod elixir_ls;
pub mod elm_language_server;
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

/// Trait for installing development tools and language servers.
pub trait Installer: Sync + Send {
    /// Returns the binary name of the tool to install.
    fn bin_name(&self) -> &'static str;

    /// Installs the tool to the configured location.
    fn install(&self) -> color_eyre::Result<()>;

    /// Checks if the tool is installed correctly.
    fn check(&self) -> Option<color_eyre::Result<String>> {
        let check_args = self.check_args()?;
        let mut cmd = Command::new(self.bin_name());
        cmd.args(check_args);

        let check_res = cmd
            .exec()
            .and_then(|output| {
                std::str::from_utf8(&output.stdout)
                    .map(ToOwned::to_owned)
                    .map_err(|error| CmdError::Utf8 {
                        cmd: Cmd::from(&cmd),
                        source: error,
                    })
            })
            .map_err(From::from);

        Some(check_res)
    }

    /// Run installer: perform install + optional version check; emit exactly one colored status line (success,
    /// not-checked, check-failed, or install-failed) for early feedback.
    ///
    /// # Returns
    /// - `Ok(())` if installation succeeded and either no check performed ([`Installer::check_args`] returned `None`)
    ///   or the check succeeded.
    /// - `Err` if installation failed or (after a successful installation) the check failed.
    ///
    /// # Errors
    /// - Propagates any error from [`Installer::install`].
    /// - Propagates process / UTF-8 decoding errors surfaced through [`Installer::check`].
    ///
    /// # Assumptions
    /// - [`Installer::install`] performs all required side effects and leaves the tool runnable by invoking its
    ///   returned binary name.
    /// - [`Installer::check_args`] returns a version / health command that exits `0` on success.
    /// - Colorized output to stdout / stderr is acceptable for the invoking environment (CI tolerates ANSI sequences).
    ///
    /// # Rationale
    /// - Provide a uniform UX: attempt install, then (if supported) a lightweight smoke test (`--version` by default).
    /// - Keep per-tool specifics encapsulated in `install` / `check`; this wrapper only orchestrates and formats status
    ///   lines.
    /// - Emit immediate, colored, human-readable status line per tool (success with check output, success without
    ///   check, install failure, or check failure) for early feedback. Printing acquires the stdout/stderr lock and
    ///   briefly serializes threads; impact is negligible versus network / IO work. Return only coarse success/failure
    ///   upward.
    ///
    /// # Performance
    /// - Dominated by underlying installer work (downloads / decompression / fs). Wrapper adds trivial overhead.
    /// - Avoids extra allocations: only trims trailing newlines on successful check output.
    ///
    /// # Future Work
    /// - Return a richer enum describing phases (installed, skipped, checked) instead of only propagating errors.
    fn run(&self) -> color_eyre::Result<()> {
        self.install()
            .inspect_err(|error| {
                eprintln!(
                    "{} {} with {}",
                    "Installation failed".red().bold(),
                    self.bin_name().white().bold(),
                    format!("error={error:#?}").red().bold()
                );
            })
            .and_then(|()| {
                self.check()
                    .transpose()
                    .inspect(|check_output| {
                        if let Some(check_output) = check_output {
                            println!(
                                "{} {} with check: {}",
                                "Installed".green().bold(),
                                self.bin_name().white().bold(),
                                check_output.trim_matches(|c| c == '\n' || c == '\r').white().bold()
                            );
                        } else {
                            println!(
                                "{} {}",
                                "Installed not checked".yellow().bold(),
                                self.bin_name().white().bold()
                            );
                        }
                    })
                    .inspect_err(|error| {
                        eprintln!(
                            "{} {} with {}",
                            "Check failed".red().bold(),
                            self.bin_name().white().bold(),
                            format!("error={error:#?}").red().bold()
                        );
                    })
            })?;

        Ok(())
    }

    /// Returns arguments for version check.
    fn check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }
}
