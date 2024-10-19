use crate::Installer;
use utils::system::silent_cmd;

pub struct Taplo {
    pub bin_dir: String,
}

impl Installer for Taplo {
    fn bin_name(&self) -> &'static str {
        "taplo"
    }

    fn install(&self) -> anyhow::Result<()> {
        // Installing with `cargo` because of:
        // 1. no particular requirements
        // 2. https://github.com/tamasfe/taplo/issues/542
        silent_cmd("cargo")
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

        Ok(())
    }
}
