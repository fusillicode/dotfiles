use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::bail;
use rootcause::prelude::ResultExt;
use ytil_sys::rustup::RequestedRustToolchain;

use crate::installers::Installer;
use crate::installers::run_health_check;

/// Install a declared Cargo or Git tool from Cargo's configured install root.
///
/// Cargo's `--root` always appends `bin`, so using a private root would impose a `bin_dir` naming constraint.
/// Instead, install only the declared tool into Cargo's configured standard root and link that exact binary into
/// `bin_dir`. Do not enumerate or update other binaries already present in Cargo's home.
pub struct Cargo<'a> {
    source_bin_dir: &'a Path,
    bin_dir: &'a Path,
    bin_name: &'static str,
    source: Source,
    health_check: HealthCheck,
}

enum Source {
    Registry {
        crate_name: &'static str,
        features: Option<&'static str>,
        all_features: bool,
        locked: bool,
    },
    Git {
        repository: &'static str,
        branch: Option<&'static str>,
        locked: bool,
        requested_toolchain: Option<RequestedRustToolchain>,
        package_name: Option<&'static str>,
    },
}

#[derive(Clone, Copy)]
enum HealthCheck {
    /// Run the installed binary directly with its health-check arguments.
    Binary(&'static [&'static str]),
    /// Run a Cargo subcommand because the installed binary expects Cargo's dispatch prefix.
    CargoSubcommand(&'static str),
}

impl<'a> Cargo<'a> {
    pub const fn registry(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        crate_name: &'static str,
    ) -> Self {
        Self {
            source_bin_dir,
            bin_dir,
            bin_name,
            source: Source::Registry {
                crate_name,
                features: None,
                all_features: false,
                locked: false,
            },
            health_check: HealthCheck::Binary(&["--version"]),
        }
    }

    pub const fn registry_with_features(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        crate_name: &'static str,
        features: &'static str,
    ) -> Self {
        Self {
            source_bin_dir,
            bin_dir,
            bin_name,
            source: Source::Registry {
                crate_name,
                features: Some(features),
                all_features: false,
                locked: false,
            },
            health_check: HealthCheck::Binary(&["--version"]),
        }
    }

    pub const fn locked_registry(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        crate_name: &'static str,
    ) -> Self {
        Self {
            source_bin_dir,
            bin_dir,
            bin_name,
            source: Source::Registry {
                crate_name,
                features: None,
                all_features: false,
                locked: true,
            },
            health_check: HealthCheck::Binary(&["--version"]),
        }
    }

    pub const fn git(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        repository: &'static str,
    ) -> Self {
        Self {
            source_bin_dir,
            bin_dir,
            bin_name,
            source: Source::Git {
                repository,
                branch: None,
                locked: false,
                requested_toolchain: None,
                package_name: None,
            },
            health_check: HealthCheck::Binary(&["--version"]),
        }
    }

    pub const fn registry_with_all_features(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        crate_name: &'static str,
    ) -> Self {
        Self {
            source_bin_dir,
            bin_dir,
            bin_name,
            source: Source::Registry {
                crate_name,
                features: None,
                all_features: true,
                locked: false,
            },
            health_check: HealthCheck::Binary(&["--version"]),
        }
    }

    pub const fn registry_with_cargo_subcommand(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        crate_name: &'static str,
        cargo_subcommand: &'static str,
    ) -> Self {
        let mut installer = Self::registry(source_bin_dir, bin_dir, bin_name, crate_name);
        installer.health_check = HealthCheck::CargoSubcommand(cargo_subcommand);
        installer
    }

    pub const fn registry_with_health_check_args(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        crate_name: &'static str,
        health_check_args: &'static [&'static str],
    ) -> Self {
        let mut installer = Self::registry(source_bin_dir, bin_dir, bin_name, crate_name);
        installer.health_check = HealthCheck::Binary(health_check_args);
        installer
    }

    pub const fn nightly_git(
        source_bin_dir: &'a Path,
        bin_dir: &'a Path,
        bin_name: &'static str,
        repository: &'static str,
        branch: &'static str,
    ) -> Self {
        Self {
            source_bin_dir,
            bin_dir,
            bin_name,
            source: Source::Git {
                repository,
                branch: Some(branch),
                locked: true,
                requested_toolchain: Some(RequestedRustToolchain::Nightly(None)),
                package_name: Some(bin_name),
            },
            health_check: HealthCheck::Binary(&["--version"]),
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
        let toolchain = match &self.source {
            Source::Git {
                requested_toolchain: Some(requested_toolchain),
                ..
            } => Some(ytil_sys::rustup::find_latest_installed_rust_toolchain(
                requested_toolchain,
            )?),
            Source::Registry { .. }
            | Source::Git {
                requested_toolchain: None,
                ..
            } => None,
        };
        let mut command = ytil_cmd::silent_cmd("cargo");
        if let Some(toolchain) = toolchain {
            command.arg(format!("+{toolchain}"));
        }
        command.args(["install", "--force"]);

        match &self.source {
            Source::Registry {
                crate_name,
                features,
                all_features,
                locked,
            } => {
                command.arg(*crate_name);
                if let Some(features) = features {
                    command.args(["--features", *features]);
                }
                if *all_features {
                    command.arg("--all-features");
                }
                if *locked {
                    command.arg("--locked");
                }
            }
            Source::Git {
                repository,
                branch,
                locked,
                package_name,
                ..
            } => {
                command.args(["--git", *repository]);
                if let Some(branch) = branch {
                    command.args(["--branch", *branch]);
                }
                if *locked {
                    command.arg("--locked");
                }
                if let Some(package_name) = package_name {
                    command.arg(*package_name);
                }
            }
        }

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
        ytil_sys::file::ln_sf(&cargo_binary, &self.bin_dir.join(self.bin_name()))?;
        ytil_sys::file::chmod_x(cargo_binary)?;

        Ok(())
    }

    fn health_check(&self) -> Option<rootcause::Result<String>> {
        let command = self.health_check_command();
        Some(run_health_check(command))
    }
}

impl Cargo<'_> {
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

/// Resolve Cargo's binary directory once before installers begin running.
pub fn bin_dir() -> rootcause::Result<PathBuf> {
    Ok(cargo_install_root()?.join("bin"))
}

/// Resolve Cargo's install-root precedence without scanning installed packages or invoking Cargo.
fn cargo_install_root() -> rootcause::Result<PathBuf> {
    if let Some(install_root) = std::env::var_os("CARGO_INSTALL_ROOT") {
        return Ok(PathBuf::from(install_root));
    }

    let cargo_home = cargo_home()?;
    if let Some(install_root) = cargo_config_install_root(&cargo_home)? {
        return Ok(install_root);
    }

    Ok(cargo_home)
}

fn cargo_home() -> rootcause::Result<PathBuf> {
    if let Some(cargo_home) = std::env::var_os("CARGO_HOME") {
        return Ok(PathBuf::from(cargo_home));
    }

    ytil_sys::dir::build_home_path(&[".cargo"])
}

/// Read `[install].root` from Cargo's standard home configuration file.
fn cargo_config_install_root(cargo_home: &Path) -> rootcause::Result<Option<PathBuf>> {
    let config_path = cargo_home.join("config.toml");
    if !config_path.is_file() {
        return Ok(None);
    }
    let config = std::fs::read_to_string(&config_path)
        .context("could not read Cargo configuration")
        .attach_with(|| format!("config={}", config_path.display()))?;
    let config: toml::Value = toml::from_str(&config)
        .context("could not parse Cargo configuration")
        .attach_with(|| format!("config={}", config_path.display()))?;
    let Some(install_root) = config
        .get("install")
        .and_then(toml::Value::as_table)
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

    #[test]
    fn test_cargo_health_check_command_when_cargo_subcommand_is_selected_uses_cargo_dispatch() {
        let source_bin_dir = Path::new("/source/bin");
        let bin_dir = Path::new("/target/bin");
        let installer = Cargo::registry_with_cargo_subcommand(
            source_bin_dir,
            bin_dir,
            "cargo-llvm-cov",
            "cargo-llvm-cov",
            "llvm-cov",
        );

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
        let installer = Cargo::registry_with_health_check_args(source_bin_dir, bin_dir, "pv", "pv", &["--help"]);

        let command = installer.health_check_command();

        assert_eq!(command.get_program(), bin_dir.join("pv"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), vec![OsStr::new("--help")]);
    }
}
