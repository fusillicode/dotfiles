use std::path::Path;

use ytil_sys::file;

use crate::Installer;

pub struct VsCodeLangServers<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for VsCodeLangServers<'_> {
    fn bin_name(&self) -> &'static str {
        "vscode-langservers-extracted"
    }

    fn install(&self) -> rootcause::Result<()> {
        let target_dir = crate::downloaders::npm::run(self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        file::ln_sf_files_in_dir(target_dir, (&self.bin_dir).into())?;
        file::chmod_x_files_in_dir(self.bin_dir)?;

        Ok(())
    }

    // NOTE: skip because it's a shitshow...
    fn health_check_args(&self) -> Option<&[&str]> {
        None
    }
}
