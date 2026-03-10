use std::path::Path;

use rootcause::prelude::ResultExt as _;

use crate::Installer;

pub struct HarperLs<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for HarperLs<'_> {
    fn bin_name(&self) -> &'static str {
        "harper-ls"
    }

    fn install(&self) -> rootcause::Result<()> {
        // Installing with `cargo` because I like it this way
        ytil_cmd::silent_cmd("cargo")
            .args([
                "install",
                self.bin_name(),
                "--force",
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
