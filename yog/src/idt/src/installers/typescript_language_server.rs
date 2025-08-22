use std::path::Path;

use crate::Installer;

pub struct TypescriptLanguageServer<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl<'a> Installer for TypescriptLanguageServer<'a> {
    fn bin_name(&self) -> &'static str {
        "typescript-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir =
            crate::downloaders::npm::run(self.dev_tools_dir, self.bin_name(), &[self.bin_name(), "typescript"])?;

        let target = target_dir.join(self.bin_name());
        utils::system::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        utils::system::chmod_x(target)?;

        Ok(())
    }
}
