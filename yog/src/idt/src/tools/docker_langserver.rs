use crate::Installer;

pub struct DockerLangServer {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for DockerLangServer {
    fn bin_name(&self) -> &'static str {
        "docker-langserver"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::downloaders::npm::run(
            &self.dev_tools_dir,
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
            &self.bin_dir,
            self.bin_name(),
        )
    }
}
