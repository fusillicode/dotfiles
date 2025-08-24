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
///
/// This trait defines the interface that all installers must implement.
/// It provides a standardized way to install tools, check their installation,
/// and report results. All installers must be thread-safe (Sync + Send).
pub trait Installer: Sync + Send {
    /// Returns the binary name of the tool to install.
    ///
    /// This is the name that will be used to invoke the tool after installation
    /// and for creating symlinks in the bin directory.
    ///
    /// # Returns
    ///
    /// The binary name as a static string slice.
    fn bin_name(&self) -> &'static str;

    /// Installs the tool to the configured location.
    ///
    /// This method handles the complete installation process for the tool,
    /// including downloading, extracting, and setting up the necessary files.
    /// The installation should be idempotent - running it multiple times
    /// should not cause issues.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if installation succeeds, or an error if it fails.
    fn install(&self) -> color_eyre::Result<()>;

    /// Checks if the tool is installed correctly by running a version check.
    ///
    /// This method runs a command to verify that the tool was installed correctly.
    /// By default, it runs `{bin_name} --version` and captures the output.
    /// Implementors can override this behavior by providing custom check logic.
    ///
    /// # Returns
    ///
    /// Returns `Some(Ok(version_string))` if the check passes and version info is available,
    /// `Some(Err(error))` if the check fails, or `None` if no check should be performed.
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

    /// Runs the installer and performs post-installation checks.
    ///
    /// This method orchestrates the complete installation and verification process:
    /// 1. Calls `install()` to install the tool
    /// 2. Calls `check()` to verify the installation
    /// 3. Reports results with appropriate success/error messages
    ///
    /// The method provides immediate feedback during installation rather than
    /// waiting for all tools to complete.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if both installation and checks succeed, or an error if either fails.
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

    /// Returns the arguments to use for the version check command.
    ///
    /// This method provides the command-line arguments that will be passed
    /// to the tool to check its version. By default, it returns `["--version"]`,
    /// but implementors can override this for tools that use different flags.
    ///
    /// # Returns
    ///
    /// Returns `Some(args)` with the check arguments, or `None` if no check should be performed.
    fn check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }
}
