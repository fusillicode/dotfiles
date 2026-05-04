use std::path::Path;

use rootcause::prelude::ResultExt;

use crate::Installer;

pub struct Taplo<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for Taplo<'_> {
    fn bin_name(&self) -> &'static str {
        "taplo"
    }

    fn install(&self) -> rootcause::Result<()> {
        // Installing with `cargo` because of:
        // 1. no particular requirements
        // 2. https://github.com/tamasfe/taplo/issues/542
        ytil_cmd::silent_cmd("cargo")
            .args([
                "install",
                &format!("{}-cli", self.bin_name()),
                "--force",
                "--all-features",
                "--root",
                // `--root` automatically append `bin` 🥲
                self.bin_dir.to_string_lossy().trim_end_matches("bin"),
            ])
            .status()
            .context("failed to spawn cargo install")?
            .exit_ok()
            .context("cargo install failed")
            .attach_with(|| format!("tool={}", self.bin_name()))?;

        ytil_sys::file::chmod_x(self.bin_dir.join(self.bin_name()))?;

        Ok(())
    }
}
