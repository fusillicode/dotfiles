pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::npm_install::run(
        dev_tools_dir,
        "typescript-language-server",
        &["typescript-language-server", "typescript"],
        bin_dir,
        "typescript-language-server",
    )
}
