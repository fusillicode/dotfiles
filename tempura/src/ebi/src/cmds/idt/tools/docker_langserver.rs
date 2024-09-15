use crate::cmds::idt::tools::Installer;

pub struct DockerLangServerInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for DockerLangServerInstaller {
    fn bin(&self) -> &'static str {
        "docker-langserver"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::npm_install::run(
            &self.dev_tools_dir,
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
            &self.bin_dir,
            self.bin(),
        )
    }
}
