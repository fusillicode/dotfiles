use crate::cmds::install_dev_tools::curl_install::OutputOption;

pub fn install(bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::curl_install::run(
        "https://github.com/mrjosh/helm-ls/releases/latest/download/helm_ls_darwin_amd64",
        OutputOption::WriteTo(&format!("{bin_dir}/helm_ls")),
    )
}
