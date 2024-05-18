use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct RustAnalyzerInstaller {
    pub bin_dir: String,
}

impl Installer for RustAnalyzerInstaller {
    fn tool(&self) -> &'static str {
        "rust_analyzer"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::curl_install::run(
       "https://github.com/rust-lang/rust-analyzer/releases/download/nightly/rust-analyzer-aarch64-apple-darwin.gz",
            OutputOption::UnpackVia(Command::new("zcat"), &format!("{}/rust-analyzer", self.bin_dir))
        )
    }
}
