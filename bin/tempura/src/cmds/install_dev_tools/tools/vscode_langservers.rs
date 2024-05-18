use crate::cmds::install_dev_tools::tools::Installer;

pub struct VsCodeLangServersInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for VsCodeLangServersInstaller {
    fn tool(&self) -> &'static str {
        "vscode_langservers"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "vscode-langservers-extracted",
            &["vscode-langservers-extracted"],
            &self.bin_dir,
            "*",
        )
    }
}
