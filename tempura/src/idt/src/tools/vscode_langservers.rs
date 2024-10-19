use crate::Installer;

pub struct VsCodeLangServersInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for VsCodeLangServersInstaller {
    fn bin(&self) -> &'static str {
        "vscode-langservers-extracted"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin(),
            &[self.bin()],
            &self.bin_dir,
            "*",
        )
    }
}
