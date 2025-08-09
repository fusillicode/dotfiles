use crate::ToolInstaller;
use crate::tools::NeedSymlink;

pub struct DockerLangServer {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for DockerLangServer {
    fn bin_name(&self) -> &'static str {
        "docker-langserver"
    }

    fn download(&self) -> color_eyre::Result<Option<NeedSymlink>> {
        let bin_src = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            "dockerfile-language-server-nodejs",
            &["dockerfile-language-server-nodejs"],
            self.bin_name(),
        )?;

        Ok(Some(NeedSymlink {
            src: bin_src.into(),
            dest: self.bin_dest_dir.clone().into(),
        }))
    }
}
