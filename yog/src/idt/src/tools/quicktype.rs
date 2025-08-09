use crate::ToolInstaller;

pub struct Quicktype {
    pub dev_tools_dir: String,
    pub bin_target_dir: String,
}

impl ToolInstaller for Quicktype {
    fn bin_name(&self) -> &'static str {
        "quicktype"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_target_dir,
            self.bin_name(),
        )
    }
}
