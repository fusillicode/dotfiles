use crate::cmds::install_dev_tools::tools::Installer;

pub struct SqlFluffInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for SqlFluffInstaller {
    fn tool(&self) -> &'static str {
        "sqlfluff"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::pip_install::run(
            &self.dev_tools_dir,
            "sqlfluff",
            &["sqlfluff"],
            &self.bin_dir,
            "sqlfluf",
        )
    }
}
