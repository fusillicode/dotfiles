use std::path::PathBuf;

use crate::Installer;

pub struct Commitlint {
    pub dev_tools_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl Installer for Commitlint {
    fn bin_name(&self) -> &'static str {
        "commitlint"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[
                &format!("@{}/cli", self.bin_name()),
                &format!("@{}/config-conventional", self.bin_name()),
            ],
        )?;

        let target = target_dir.join(self.bin_name());
        utils::system::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        utils::system::chmod_x(target)?;

        Ok(())
    }
}
