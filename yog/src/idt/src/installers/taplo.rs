use crate::Installer;

pub struct Taplo {
    pub bin_dir: String,
}

impl Installer for Taplo {
    fn bin_name(&self) -> &'static str {
        "taplo"
    }

    fn install(&self) -> color_eyre::Result<()> {
        // Installing with `cargo` because of:
        // 1. no particular requirements
        // 2. https://github.com/tamasfe/taplo/issues/542
        utils::cmd::silent_cmd("cargo")
            .args([
                "install",
                &format!("{}-cli", self.bin_name()),
                "--force",
                "--all-features",
                "--root",
                // `--root` automatically append `bin` 🥲
                self.bin_dir.trim_end_matches("bin"),
            ])
            .status()?;

        utils::system::chmod_x(format!("{}/{}", self.bin_dir, self.bin_name()))?;

        Ok(())
    }
}
