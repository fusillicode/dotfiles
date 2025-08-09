use crate::Installer;

pub struct GraphQlLsp {
    pub dev_tools_dir: String,
    pub bin_target_dir: String
}

impl Installer for GraphQlLsp {
    fn bin_name(&self) -> &'static str {
        "graphql-lsp"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
            &self.bin_target_dir,
            self.bin_name(),
        )
    }
}
