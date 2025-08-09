use crate::ToolInstaller;
use crate::tools::NeedSymlink;

pub struct VsCodeLangServers {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for VsCodeLangServers {
    fn bin_name(&self) -> &'static str {
        "vscode-langservers-extracted"
    }

    fn download(&self) -> color_eyre::Result<Option<NeedSymlink>> {
        let bin_src = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            "*",
        )?;

        Ok(Some(NeedSymlink {
            src: bin_src.into(),
            dest: self.bin_dest_dir.clone().into(),
        }))
    }
}
