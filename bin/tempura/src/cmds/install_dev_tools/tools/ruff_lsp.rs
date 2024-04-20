pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::pip_install::run(
        dev_tools_dir,
        "ruff-lsp",
        &["ruff-lsp"],
        bin_dir,
        "ruff-lsp",
    )
}
