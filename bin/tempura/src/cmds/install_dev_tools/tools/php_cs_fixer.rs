pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    crate::cmds::install_dev_tools::composer_install::run(
        dev_tools_dir,
        "php-cs-fixer",
        &["friendsofphp/php-cs-fixer"],
        bin_dir,
        "php-cs-fixer",
    )
}
