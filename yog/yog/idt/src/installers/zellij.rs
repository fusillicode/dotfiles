use std::path::Path;

use crate::Installer;

pub struct Zellij<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for Zellij<'_> {
    fn bin_name(&self) -> &'static str {
        "zellij"
    }

    fn install(&self) -> rootcause::Result<()> {
        ytil_cmd::silent_cmd("cargo")
            .args([
                "install",
                self.bin_name(),
                "--force",
                "--root",
                self.bin_dir.to_string_lossy().trim_end_matches("bin"),
            ])
            .status()?
            .exit_ok()?;

        ytil_sys::file::chmod_x(self.bin_dir.join(self.bin_name()))?;

        Ok(())
    }
}
