pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::npm_install::run(
        dev_tools_dir,
        "dockerfile-language-server-nodejs",
        &["dockerfile-language-server-nodejs"],
        bin_dir,
        "docker-langserver",
    )
}
