use std::path::Path;

use rootcause::prelude::ResultExt as _;
use ytil_cmd::CmdExt as _;
use ytil_cmd::silent_cmd;

use crate::Installer;

pub struct Starship<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for Starship<'_> {
    fn bin_name(&self) -> &'static str {
        "starship"
    }

    fn install(&self) -> rootcause::Result<()> {
        let source_dir = self.dev_tools_dir.join(self.bin_name()).join("source");
        let cargo_target = self.dev_tools_dir.join("cargo-target");

        silent_cmd("sh")
            .args([
                "-c",
                &format!(
                    r#"
                        ([ ! -d "{0}" ] && \
                            git clone --depth=1 https://github.com/starship/starship.git {0} || true) && \
                        cd {0} && \
                        git fetch origin master --depth=1 && \
                        git checkout origin/master && \
                        CARGO_TARGET_DIR={1} cargo build --release
                    "#,
                    source_dir.display(),
                    cargo_target.display(),
                ),
            ])
            .status()
            .context("failed to spawn build command")?
            .exit_ok()
            .context("build failed")
            .attach_with(|| format!("tool={}", self.bin_name()))
            .attach_with(|| format!("source_dir={}", source_dir.display()))?;

        let target = cargo_target.join("release").join(self.bin_name());
        ytil_sys::file::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }

    fn health_check(&self) -> Option<rootcause::Result<String>> {
        // Starship refuses to run under TERM=dumb.
        // Override TERM so the health check succeeds regardless of the host terminal.
        let res = std::process::Command::new(self.bin_name())
            .env("TERM", "xterm-256color")
            .args(["--version"])
            .exec()
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .map_err(From::from);
        Some(res)
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }
}
