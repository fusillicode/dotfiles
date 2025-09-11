use std::process::Command;

use cmd::CmdDetails;
use cmd::CmdError;
use cmd::CmdExt;
use color_eyre::owo_colors::OwoColorize as _;

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
                        cmd_details: CmdDetails::from(&cmd),
                        source: error,
                    })
            })
            .map_err(From::from);

        Some(check_res)
    }

    /// Runs the installer and checks installation.
    fn run(&self) -> color_eyre::Result<()> {
        self.install()
            .inspect_err(|error| {
                eprintln!(
                    "{} {} with error: {}",
                    "Installation failed".red().bold(),
                    self.bin_name().bold(),
                    format!("{error:#?}").red().bold()
                );
            })
            .and_then(|()| {
                self.check()
                    .transpose()
                    .inspect(|check_output| {
                        if let Some(check_output) = check_output {
                            println!(
                                "{} {} with output: {}",
                                "Installed & checked".green().bold(),
                                self.bin_name().bold(),
                                check_output.trim_matches(|c| c == '\n' || c == '\r').bold()
                            );
                        } else {
                            println!("{} {}", "Installed not checked".yellow().bold(), self.bin_name().bold());
                        }
                    })
                    .inspect_err(|error| {
                        eprintln!(
                            "{} {} with error: {}",
                            "Installed & check failed".red().bold(),
                            self.bin_name().bold(),
                            format!("{error:#?}").red().bold()
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
