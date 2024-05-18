use crate::cmds::install_dev_tools::tools::Installer;

pub struct DockerLangServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for DockerLangServerInstaller {
    fn tool(&self) -> &'static str {
        "docker_langserver"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::install_dev_tools::npm_install::run(
            &self.dev_tools_dir,
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
            &self.bin_dir,
            "docker-langserver",
        )
    }
}
