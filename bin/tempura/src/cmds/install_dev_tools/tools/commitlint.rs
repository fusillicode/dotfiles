use crate::cmds::install_dev_tools::tools::Installer;

pub struct CommitlintInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for CommitlintInstaller {
    fn bin(&self) -> &'static str {
        "commitlint"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[
                &format!("@{}/cli", self.bin()),
                &format!("@{}/config-conventional", self.bin()),
            ],
            &self.bin_dir,
            self.bin(),
        )
    }
}
