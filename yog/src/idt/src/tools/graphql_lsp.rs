use crate::ToolInstaller;

pub struct GraphQlLsp {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for GraphQlLsp {
    fn bin_name(&self) -> &'static str {
        "graphql-lsp"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
            &self.bin_dest_dir,
            self.bin_name(),
        )
    }
}
