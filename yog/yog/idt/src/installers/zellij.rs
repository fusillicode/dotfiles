use std::path::Path;

use rootcause::prelude::ResultExt as _;
use ytil_cmd::silent_cmd;

use crate::Installer;

pub struct Zellij<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for Zellij<'_> {
    fn bin_name(&self) -> &'static str {
        "zellij"
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
                            git clone --depth=1 https://github.com/zellij-org/zellij.git {0} || true) && \
                        cd {0} && \
                        git fetch origin main --depth=1 && \
                        git checkout origin/main && \
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

    fn should_verify_checksum(&self) -> bool {
        false
    }
}
