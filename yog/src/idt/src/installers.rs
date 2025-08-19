use std::process::Command;

use utils::cmd::CmdDetails;
use utils::cmd::CmdError;
use utils::cmd::CmdExt;

pub mod bash_language_server;
pub mod commitlint;
pub mod deno;
pub mod docker_langserver;
pub mod elixir_ls;
pub mod elm_language_server;
pub mod eslint_d;
pub mod graphql_lsp;
pub mod hadolint;
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

pub trait Installer: Sync + Send {
    fn bin_name(&self) -> &'static str;

    fn install(&self) -> color_eyre::Result<()>;

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

    // Reporting is done here, rather than after via `installers_errors`, to report
    // results as soon as possible.
    fn run(&self) -> color_eyre::Result<()> {
        self.install()
            .inspect_err(|error| eprintln!("❌ {} installation failed, error {error:#?}", self.bin_name()))
            .and_then(|_| {
                self.check()
                    .transpose()
                    .inspect(|check_output| {
                        if let Some(check_output) = check_output {
                            println!(
                                "✅ {} installed, check output: {}",
                                self.bin_name(),
                                check_output.trim_matches(|c| c == '\n' || c == '\r')
                            );
                        } else {
                            println!("🎲 {} installed, check skipped", self.bin_name());
                        };
                    })
                    .inspect_err(|error| eprintln!("❌ {} check failed, error {error:#?}", self.bin_name()))
            })?;

        Ok(())
    }

    fn check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }
}
