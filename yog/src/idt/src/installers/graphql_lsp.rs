use std::path::PathBuf;

use crate::Installer;

pub struct GraphQlLsp {
    pub dev_tools_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl Installer for GraphQlLsp {
    fn bin_name(&self) -> &'static str {
        "graphql-lsp"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
        )?;

        let target = target_dir.join(self.bin_name());
        utils::system::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        utils::system::chmod_x(target)?;

        Ok(())
    }
}
