use std::path::PathBuf;

use crate::Installer;

pub struct DockerLangServer {
    pub dev_tools_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl Installer for DockerLangServer {
    fn bin_name(&self) -> &'static str {
        "docker-langserver"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
        )?;

        let target = target_dir.join(self.bin_name());
        utils::system::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        utils::system::chmod_x(target)?;

        Ok(())
    }

    // NOTE: skip because JS is a shitshow...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }
}
