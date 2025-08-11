use crate::Installer;

pub struct GraphQlLsp {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for GraphQlLsp {
    fn bin_name(&self) -> &'static str {
        "graphql-lsp"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::downloaders::npm::run(
            &self.dev_tools_dir,
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
            &self.bin_dir,
            self.bin_name(),
        )?;

        Ok(())
    }
}
