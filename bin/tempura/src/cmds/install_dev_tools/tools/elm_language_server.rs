pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::npm_install::run(
        dev_tools_dir,
        "elm-language-server",
        &["@elm-tooling/elm-language-server"],
        bin_dir,
        "elm-language-server",
    )
}
