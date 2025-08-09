use crate::ToolInstaller;
use crate::tools::NeedSymlink;

pub struct BashLanguageServer {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for BashLanguageServer {
    fn bin_name(&self) -> &'static str {
        "bash-language-server"
    }

    fn download(&self) -> color_eyre::Result<NeedSymlink> {
        let bin_src_dir =
            crate::downloaders::npm::run(&self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        Ok(NeedSymlink::Yes {
            src: format!("{bin_src_dir}/{}", self.bin_name()).into(),
            dest: self.bin_dest_dir.clone().into(),
        })
    }
}
