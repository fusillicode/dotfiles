use std::path::Path;

use crate::Installer;

pub struct TypescriptLanguageServer<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for TypescriptLanguageServer<'_> {
    fn bin_name(&self) -> &'static str {
        "typescript-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir =
            crate::downloaders::npm::run(self.dev_tools_dir, self.bin_name(), &[self.bin_name(), "typescript"])?;

        let target = target_dir.join(self.bin_name());
        ytil_sys::file::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
