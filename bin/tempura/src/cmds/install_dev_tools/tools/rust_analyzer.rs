use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;

pub fn install(bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::curl_install::run(
        "https://github.com/rust-lang/rust-analyzer/releases/download/nightly/rust-analyzer-aarch64-apple-darwin.gz",
        OutputOption::UnpackVia(Command::new("zcat"), &format!("{bin_dir}/rust-analyzer"))
    )
}
