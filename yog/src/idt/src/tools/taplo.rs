use crate::ToolInstaller;
use crate::tools::NeedSymlink;

pub struct Taplo {
    pub bin_dest_dir: String,
}

impl ToolInstaller for Taplo {
    fn bin_name(&self) -> &'static str {
        "taplo"
    }

    fn download(&self) -> color_eyre::Result<Option<NeedSymlink>> {
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
                self.bin_dest_dir.trim_end_matches("bin"),
            ])
            .status()?;

        Ok(None)
    }
}
