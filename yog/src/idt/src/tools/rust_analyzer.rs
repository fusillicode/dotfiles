use std::process::Command;

use crate::Installer;
use crate::installers::curl_install::OutputOption;

pub struct RustAnalyzer {
    pub bin_target_dir: String,
}

impl Installer for RustAnalyzer {
    fn bin_name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::curl_install::run(
            &format!(
                "https://github.com/rust-lang/{0}/releases/download/nightly/{0}-aarch64-apple-darwin.gz",
                self.bin_name()
            ),
            OutputOption::UnpackVia(
                Box::new(Command::new("zcat")),
                &format!("{}/{}", self.bin_target_dir, self.bin_name()),
            ),
        )
    }
}
