use crate::Installer;

pub struct VsCodeLangServers {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for VsCodeLangServers {
    fn bin_name(&self) -> &'static str {
        "vscode-langservers-extracted"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir = crate::downloaders::npm::run(&self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        utils::system::ln_sf_files_in_dir(target_dir, (&self.bin_dir).into())?;
        utils::system::chmod_x_files_in_dir(&self.bin_dir)?;

        Ok(())
    }

    // NOTE: skip because it's a shitshow...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }
}
