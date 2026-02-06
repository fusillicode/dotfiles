use std::path::Path;

use crate::Installer;

pub struct DockerLangServer<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for DockerLangServer<'_> {
    fn bin_name(&self) -> &'static str {
        "docker-langserver"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::installers::install_npm_tool(
            self.dev_tools_dir,
            self.bin_dir,
            self.bin_name(),
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
        )
    }

    // NOTE: skip because JS is a shitshow...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }
}
