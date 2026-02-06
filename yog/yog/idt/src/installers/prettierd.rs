use std::path::Path;

use crate::Installer;

pub struct PrettierD<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for PrettierD<'_> {
    fn bin_name(&self) -> &'static str {
        "prettierd"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::install_npm_tool(
            self.dev_tools_dir,
            self.bin_dir,
            self.bin_name(),
            self.bin_name(),
            &[&format!("@fsouza/{}", self.bin_name())],
        )
    }
}
