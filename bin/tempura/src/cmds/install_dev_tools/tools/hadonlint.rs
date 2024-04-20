use crate::cmds::install_dev_tools::curl_install::OutputOption;

pub fn install(bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::curl_install::run(
        "https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Darwin-x86_64",
        OutputOption::WriteTo(&format!("{bin_dir}/hadolint")),
    )
}
