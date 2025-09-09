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
        let target_dir = crate::downloaders::npm::run(
            self.dev_tools_dir,
            self.bin_name(),
            &[&format!("@fsouza/{}", self.bin_name())],
        )?;

        let target = target_dir.join(self.bin_name());
        system::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        system::chmod_x(target)?;

        Ok(())
    }
}
