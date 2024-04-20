pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::npm_install::run(
        dev_tools_dir,
        "bash-language-server",
        &["bash-language-server"],
        bin_dir,
        "bash-language-server",
    )
}
