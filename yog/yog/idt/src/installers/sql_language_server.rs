use std::path::Path;

use crate::Installer;

pub struct SqlLanguageServer<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for SqlLanguageServer<'_> {
    fn bin_name(&self) -> &'static str {
        "sql-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir = crate::downloaders::npm::run(self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        let target = target_dir.join(self.bin_name());
        ytil_sys::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        ytil_sys::chmod_x(target)?;

        Ok(())
    }
}
