use crate::Installer;

pub struct HarperLs {
    pub bin_dir: String,
}

impl Installer for HarperLs {
    fn bin_name(&self) -> &'static str {
        "harper-ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        // Installing with `cargo` because I like it this way
        utils::cmd::silent_cmd("cargo")
            .args([
                "install",
                self.bin_name(),
                "--force",
                "--root",
                // `--root` automatically append `bin` 🥲
                self.bin_dir.trim_end_matches("bin"),
            ])
            .status()?;

        utils::system::chmod_x(format!("{}/{}", self.bin_dir, self.bin_name()))?;

        Ok(())
    }
}
