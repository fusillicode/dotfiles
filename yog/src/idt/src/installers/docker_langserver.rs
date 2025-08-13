use utils::system::symlink::Symlink;

use crate::Installer;

pub struct DockerLangServer {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for DockerLangServer {
    fn bin_name(&self) -> &'static str {
        "docker-langserver"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
        )?;

        let target = format!("{target_dir}/{}", self.bin_name());
        let symlink = utils::system::symlink::build(&target, Some(&self.bin_dir))?;

        Ok(symlink)
    }
}
