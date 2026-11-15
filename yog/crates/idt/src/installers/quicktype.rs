use std::path::Path;

use crate::Installer;
use crate::installers::InstallOutcome;

pub struct Quicktype<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for Quicktype<'_> {
    fn bin_name(&self) -> &'static str {
        "quicktype"
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
