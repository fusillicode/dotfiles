pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::npm_install::run(
        dev_tools_dir,
        "vscode-langservers-extracted",
        &["vscode-langservers-extracted"],
        bin_dir,
        "*",
    )
}
