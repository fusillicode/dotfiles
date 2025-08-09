use crate::ToolInstaller;

pub struct DockerLangServer {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for DockerLangServer {
    fn bin_name(&self) -> &'static str {
        "docker-langserver"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::npm_install::run(
            &self.dev_tools_dir,
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
            &self.bin_dest_dir,
            self.bin_name(),
        )
    }
}
