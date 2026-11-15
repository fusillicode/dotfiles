use std::path::Path;

use crate::installer::InstallOutcome;
use crate::installer::Installer;

pub struct GraphQlLsp<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for GraphQlLsp<'_> {
    fn bin_name(&self) -> &'static str {
        "graphql-lsp"
    }

    fn install(&self) -> rootcause::Result<InstallOutcome> {
        crate::installers::install_npm_tool(
            self.dev_tools_dir,
            self.bin_dir,
            self.bin_name(),
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
        )
    }
}
