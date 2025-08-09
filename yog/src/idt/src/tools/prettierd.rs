use crate::ToolInstaller;

pub struct PrettierD {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for PrettierD {
    fn bin_name(&self) -> &'static str {
        "prettierd"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[&format!("@fsouza/{}", self.bin_name())],
            &self.bin_dest_dir,
            self.bin_name(),
        )
    }
}
