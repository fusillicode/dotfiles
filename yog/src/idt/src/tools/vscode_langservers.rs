
use crate::Installer;

pub struct VsCodeLangServers {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for VsCodeLangServers {
    fn bin_name(&self) -> &'static str {
        "vscode-langservers-extracted"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_dir,
            "*",
        )
    }
}
