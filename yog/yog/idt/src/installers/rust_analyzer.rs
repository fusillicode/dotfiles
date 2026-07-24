use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::prelude::ResultExt;

use crate::installers::Installer;
use crate::installers::run_health_check;

pub struct RustAnalyzer<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for RustAnalyzer<'_> {
    fn bin_name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn install(&self) -> rootcause::Result<()> {
        ytil_cmd::silent_cmd("cargo")
            .args([
                "+nightly",
                "install",
                "--git",
                "https://github.com/rust-lang/rust-analyzer.git",
                "--branch",
                "master",
                "--locked",
                "--force",
                self.bin_name(),
            ])
            .status()
            .context("failed to spawn cargo install")?
            .exit_ok()
            .context("cargo install failed")
            .attach_with(|| format!("tool={}", self.bin_name()))?;

        let cargo_binary = cargo_bin_dir()
            .map(|bin_dir| bin_dir.join(self.bin_name()))?
            .canonicalize()
            .context("could not resolve Cargo-installed rust-analyzer")?;
        ytil_sys::file::ln_sf(&cargo_binary, &self.bin_dir.join(self.bin_name()))?;

        Ok(())
    }

    fn health_check(&self) -> Option<rootcause::Result<String>> {
        let args = self.health_check_args()?;
        let result = cargo_bin_dir().map(|bin_dir| {
            let mut cmd = Command::new(bin_dir.join(self.bin_name()));
            cmd.args(args);
            cmd
        });

        Some(result.and_then(run_health_check))
    }
}

fn cargo_bin_dir() -> rootcause::Result<PathBuf> {
    if let Some(cargo_home) = std::env::var_os("CARGO_HOME") {
        let cargo_home = PathBuf::from(cargo_home);
        return Ok(cargo_bin_dir_for(Some(&cargo_home), Path::new("")));
    }

    ytil_sys::dir::build_home_path(&[".cargo", "bin"])
}

fn cargo_bin_dir_for(cargo_home: Option<&Path>, home_dir: &Path) -> PathBuf {
    cargo_home
        .map_or_else(|| home_dir.join(".cargo"), Path::to_path_buf)
        .join("bin")
}
