use std::path::Path;

use crate::Installer;
use crate::installers::InstallOutcome;

pub struct Commitlint<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for Commitlint<'_> {
    fn bin_name(&self) -> &'static str {
        "commitlint"
    }

    fn install(&self) -> rootcause::Result<InstallOutcome> {
        crate::installers::install_npm_tool(
            self.dev_tools_dir,
            self.bin_dir,
            self.bin_name(),
            self.bin_name(),
            &[
                &format!("@{}/cli", self.bin_name()),
                &format!("@{}/config-conventional", self.bin_name()),
            ],
        )
    }
}
