use crate::cmds::install_dev_tools::curl_install::OutputOption;

pub fn install(bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::curl_install::run(
        "https://github.com/artempyanykh/marksman/releases/latest/download/marksman-macos",
        OutputOption::WriteTo(&format!("{bin_dir}/marksman")),
    )
}
