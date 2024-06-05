use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct RustAnalyzerInstaller {
    pub bin_dir: String,
}

impl Installer for RustAnalyzerInstaller {
    fn bin(&self) -> &'static str {
        "rust-analyzer"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::curl_install::run(
           &format!("https://github.com/rust-lang/{0}/releases/download/nightly/{0}-aarch64-apple-darwin.gz", self.bin()),
           OutputOption::UnpackVia(Command::new("zcat"), &format!("{}/{}", self.bin_dir, self.bin()))
        )
    }
}
