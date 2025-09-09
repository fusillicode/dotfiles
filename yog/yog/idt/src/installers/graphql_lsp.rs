use std::path::Path;

use crate::Installer;

pub struct GraphQlLsp<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for GraphQlLsp<'_> {
    fn bin_name(&self) -> &'static str {
        "graphql-lsp"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir = crate::downloaders::npm::run(
            self.dev_tools_dir,
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
        )?;

        let target = target_dir.join(self.bin_name());
        system::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        system::chmod_x(target)?;

        Ok(())
    }
}
