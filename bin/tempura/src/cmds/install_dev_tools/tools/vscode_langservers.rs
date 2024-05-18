use crate::cmds::install_dev_tools::tools::Installer;

pub struct VsCodeLangServersInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for VsCodeLangServersInstaller {
    fn bin(&self) -> &'static str {
        "vscode-langservers-extracted"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[self.bin()],
            &self.bin_dir,
            "*",
        )
    }
}
