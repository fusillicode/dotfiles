use std::path::PathBuf;

use crate::Installer;

pub struct ElmLanguageServer {
    pub dev_tools_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl Installer for ElmLanguageServer {
    fn bin_name(&self) -> &'static str {
        "elm-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[&format!("@elm-tooling/{}", self.bin_name())],
        )?;

        let target = target_dir.join(self.bin_name());
        utils::system::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        utils::system::chmod_x(target)?;

        Ok(())
    }
}
