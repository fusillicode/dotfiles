use crate::ToolInstaller;

pub struct BashLanguageServer {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for BashLanguageServer {
    fn bin_name(&self) -> &'static str {
        "bash-language-server"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            &self.bin_dest_dir,
            self.bin_name(),
        )
    }
}
