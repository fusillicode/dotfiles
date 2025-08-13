use utils::system::symlink::Symlink;

use crate::Installer;

pub struct Taplo {
    pub bin_dir: String,
}

impl Installer for Taplo {
    fn bin_name(&self) -> &'static str {
        "taplo"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
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

        let target = format!("{}/{}", self.bin_dir, self.bin_name());
        let symlink = utils::system::symlink::build(&target, None)?;

        Ok(symlink)
    }
}
