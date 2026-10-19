use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::bail;
use rootcause::prelude::ResultExt;
use toml::Value;
use ytil_sys::dir;
use ytil_sys::file;
use ytil_sys::rustup;
use ytil_sys::rustup::RequestedRustToolchain;

use crate::installers;
use crate::installers::Installer;

/// Install a declared Cargo or Git tool from Cargo's configured install root.
///
/// Cargo's `--root` always appends `bin`, so using a private root would impose a `bin_dir` naming constraint.
/// Instead, install only the declared tool into Cargo's configured standard root and link that exact binary into
/// `bin_dir`. Do not enumerate or update other binaries already present in Cargo's home.
pub struct Cargo<'a> {
    source_bin_dir: &'a Path,
    bin_dir: &'a Path,
    bin_name: &'static str,
    install_args: &'static [&'static str],
    requested_toolchain: Option<RequestedRustToolchain>,
    health_check: HealthCheck,
}

impl<'a> Cargo<'a> {
    /// Pass arguments directly to `cargo install`, after the shared `--force` flag.
    pub const fn new(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        install_args: &'static [&'static str],
    ) -> Self {
        Self {
            source_bin_dir,
            bin_dir,
            bin_name,
            install_args,
            requested_toolchain: None,
            health_check: HealthCheck::Binary(&["--version"]),
        }
    }

    pub fn with_toolchain(mut self, toolchain: RequestedRustToolchain) -> Self {
        self.requested_toolchain = Some(toolchain);
        self
    }

    pub const fn with_cargo_subcommand(mut self, subcommand: &'static str) -> Self {
        self.health_check = HealthCheck::CargoSubcommand(subcommand);
        self
    }

    pub const fn with_health_check_args(mut self, args: &'static [&'static str]) -> Self {
        self.health_check = HealthCheck::Binary(args);
        self
    }

    fn install_command(&self) -> rootcause::Result<Command> {
        let mut command = ytil_cmd::silent_cmd("cargo");
        if let Some(requested_toolchain) = &self.requested_toolchain {
            let toolchain = rustup::find_latest_installed_rust_toolchain(requested_toolchain)?;
            command.arg(format!("+{toolchain}"));
        }
        command.args(["install", "--force"]).args(self.install_args);
        Ok(command)
    }

    fn health_check_command(&self) -> Command {
        match self.health_check {
            HealthCheck::Binary(args) => {
                let mut command = Command::new(self.bin_dir.join(self.bin_name()));
                command.args(args);
                command
            }
            HealthCheck::CargoSubcommand(subcommand) => {
                let mut command = Command::new("cargo");
                command.args([subcommand, "--version"]);
                command
            }
        }
    }
}

impl Installer for Cargo<'_> {
    fn bin_name(&self) -> &'static str {
        self.bin_name
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }

    fn install(&self) -> rootcause::Result<()> {
        let mut command = self.install_command()?;

        command
            .status()
            .context("failed to spawn cargo install")?
            .exit_ok()
            .context("cargo install failed")
            .attach_with(|| format!("tool={}", self.bin_name()))
            .attach_with(|| format!("command={command:?}"))?;

        let cargo_binary = self
            .source_bin_dir
            .join(self.bin_name())
            .canonicalize()
            .context("could not resolve Cargo-installed binary")?;
        file::ln_sf(&cargo_binary, &self.bin_dir.join(self.bin_name()))?;
        file::chmod_x(cargo_binary)?;

        Ok(())
    }

    fn health_check(&self) -> Option<rootcause::Result<String>> {
        let command = self.health_check_command();
        Some(installers::run_health_check(command))
    }
}

/// Resolve Cargo's binary directory once before installers begin running.
pub fn bin_dir() -> rootcause::Result<PathBuf> {
    Ok(cargo_install_root()?.join("bin"))
}

#[derive(Clone, Copy)]
enum HealthCheck {
    /// Run the installed binary directly with its health-check arguments.
    Binary(&'static [&'static str]),
    /// Run a Cargo subcommand because the installed binary expects Cargo's dispatch prefix.
    CargoSubcommand(&'static str),
}

/// Resolve Cargo's install-root precedence without scanning installed packages or invoking Cargo.
fn cargo_install_root() -> rootcause::Result<PathBuf> {
    if let Some(install_root) = env::var_os("CARGO_INSTALL_ROOT") {
        return Ok(PathBuf::from(install_root));
    }

    let cargo_home = cargo_home()?;
    if let Some(install_root) = cargo_config_install_root(&cargo_home)? {
        return Ok(install_root);
    }

    Ok(cargo_home)
}

fn cargo_home() -> rootcause::Result<PathBuf> {
    if let Some(cargo_home) = env::var_os("CARGO_HOME") {
        return Ok(PathBuf::from(cargo_home));
    }

    dir::build_home_path(&[".cargo"])
}

/// Read `[install].root` from Cargo's standard home configuration file.
fn cargo_config_install_root(cargo_home: &Path) -> rootcause::Result<Option<PathBuf>> {
    let config_path = cargo_home.join("config.toml");
    if !config_path.is_file() {
        return Ok(None);
    }
    let config = fs::read_to_string(&config_path)
        .context("could not read Cargo configuration")
        .attach_with(|| format!("config={}", config_path.display()))?;
    let config: Value = toml::from_str(&config)
        .context("could not parse Cargo configuration")
        .attach_with(|| format!("config={}", config_path.display()))?;
    let Some(install_root) = config
        .get("install")
        .and_then(Value::as_table)
        .and_then(|install| install.get("root"))
    else {
        return Ok(None);
    };
    let Some(install_root) = install_root.as_str() else {
        bail!("install.root in {} must be a path string", config_path.display());
    };

    resolve_config_path(&config_path, install_root).map(Some)
}

fn resolve_config_path(config_path: &Path, install_root: &str) -> rootcause::Result<PathBuf> {
    let install_root = PathBuf::from(install_root);
    if install_root.is_absolute() {
        return Ok(install_root);
    }

    let Some(config_dir) = config_path.parent() else {
        bail!("Cargo configuration path has no parent: {}", config_path.display());
    };
    Ok(config_dir.join(install_root))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[rstest::rstest]
    #[case::registry(&["ripgrep"], &["install", "--force", "ripgrep"])]
    #[case::features(&["cargo-audit", "--features", "fix"], &["install", "--force", "cargo-audit", "--features", "fix"])]
    #[case::all_features(&["taplo-cli", "--all-features"], &["install", "--force", "taplo-cli", "--all-features"])]
    #[case::locked(&["cargo-nextest", "--locked"], &["install", "--force", "cargo-nextest", "--locked"])]
    #[case::git(
        &["--git", "https://github.com/rtk-ai/rtk", "--branch", "master", "rtk"],
        &["install", "--force", "--git", "https://github.com/rtk-ai/rtk", "--branch", "master", "rtk"],
    )]
    #[case::version_and_defaults(
        &["tool", "--version", "1.2.3", "--no-default-features", "--features", "cli"],
        &["install", "--force", "tool", "--version", "1.2.3", "--no-default-features", "--features", "cli"],
    )]
    #[case::path_with_spaces(
        &["--path", "/tools/my crate"],
        &["install", "--force", "--path", "/tools/my crate"],
    )]
    fn test_install_command_when_arguments_vary_preserves_cargo_options(
        #[case] args: &'static [&'static str],
        #[case] expected: &[&str],
    ) {
        let installer = Cargo::new(Path::new("/source/bin"), Path::new("/target/bin"), "tool", args);

        let command = installer.install_command().unwrap();

        assert_eq!(command.get_program(), OsStr::new("cargo"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn test_cargo_health_check_command_when_defaults_are_selected_uses_linked_binary_version() {
        let installer = Cargo::new(Path::new("/source/bin"), Path::new("/target/bin"), "rtk", &["rtk"]);

        let command = installer.health_check_command();

        assert_eq!(command.get_program(), OsStr::new("/target/bin/rtk"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), vec![OsStr::new("--version")]);
    }

    #[test]
    fn test_cargo_health_check_command_when_cargo_subcommand_is_selected_uses_cargo_dispatch() {
        let source_bin_dir = Path::new("/source/bin");
        let bin_dir = Path::new("/target/bin");
        let installer = Cargo::new(source_bin_dir, bin_dir, "cargo-llvm-cov", &["cargo-llvm-cov"])
            .with_cargo_subcommand("llvm-cov");

        let command = installer.health_check_command();

        assert_eq!(command.get_program(), OsStr::new("cargo"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("llvm-cov"), OsStr::new("--version")]
        );
    }

    #[test]
    fn test_cargo_health_check_command_when_custom_args_are_selected_uses_direct_binary() {
        let source_bin_dir = Path::new("/source/bin");
        let bin_dir = Path::new("/target/bin");
        let installer = Cargo::new(source_bin_dir, bin_dir, "pv", &["pv"]).with_health_check_args(&["--help"]);

        let command = installer.health_check_command();

        assert_eq!(command.get_program(), bin_dir.join("pv"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), vec![OsStr::new("--help")]);
    }
}
