use crate::cmds::install_dev_tools::tools::Installer;

pub struct GraphQlLspInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for GraphQlLspInstaller {
    fn bin(&self) -> &'static str {
        "graphql-lsp"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
            &self.bin_dir,
            self.bin(),
        )
    }
}
