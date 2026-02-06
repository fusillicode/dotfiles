use std::path::Path;

use crate::Installer;

pub struct YamlLanguageServer<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for YamlLanguageServer<'_> {
    fn bin_name(&self) -> &'static str {
        "yaml-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::install_npm_tool(
            self.dev_tools_dir,
            self.bin_dir,
            self.bin_name(),
            self.bin_name(),
            &[self.bin_name()],
        )
    }

    // NOTE: skip because JS is a shitshow...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }
}
