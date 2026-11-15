use std::path::Path;

use crate::Installer;
use crate::installers::InstallOutcome;

pub struct SqlLanguageServer<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for SqlLanguageServer<'_> {
    fn bin_name(&self) -> &'static str {
        "sql-language-server"
    }

    fn install(&self) -> rootcause::Result<InstallOutcome> {
        crate::installers::install_npm_tool(
            self.dev_tools_dir,
            self.bin_dir,
            self.bin_name(),
            self.bin_name(),
            &[self.bin_name()],
        )
    }
}
