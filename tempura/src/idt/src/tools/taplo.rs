use crate::Installer;
use utils::system::silent_cmd;

pub struct TaploInstaller {
    pub bin_dir: String,
}

impl Installer for TaploInstaller {
    fn bin(&self) -> &'static str {
        "taplo"
    }

    fn install(&self) -> anyhow::Result<()> {
        // Installing with `cargo` because of:
        // 1. no particular requirements
        // 2. https://github.com/tamasfe/taplo/issues/542
        silent_cmd("cargo")
            .args([
                "install",
                &format!("{}-cli", self.bin()),
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
