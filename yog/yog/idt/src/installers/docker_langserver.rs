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
        let target_dir = crate::downloaders::npm::run(
            self.dev_tools_dir,
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
        )?;

        let target = target_dir.join(self.bin_name());
        ytil_sys::file::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }

    // NOTE: skip because JS is a shitshow...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }
}
