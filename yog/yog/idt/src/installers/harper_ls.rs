use std::path::Path;

use crate::Installer;

pub struct HarperLs<'a> {
    pub bin_dir: &'a Path,
}

impl Installer for HarperLs<'_> {
    fn bin_name(&self) -> &'static str {
        "harper-ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        // Installing with `cargo` because I like it this way
        cmd::silent_cmd("cargo")
            .args([
                "install",
                self.bin_name(),
                "--force",
                "--root",
                // `--root` automatically append `bin` 🥲
                self.bin_dir.to_string_lossy().trim_end_matches("bin"),
            ])
            .status()?;

        system::chmod_x(self.bin_dir.join(self.bin_name()))?;

        Ok(())
    }
}
