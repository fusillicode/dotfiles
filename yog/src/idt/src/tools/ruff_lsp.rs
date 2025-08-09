use crate::ToolInstaller;
use crate::tools::NeedSymlink;

pub struct RuffLsp {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for RuffLsp {
    fn bin_name(&self) -> &'static str {
        "ruff-lsp"
    }

    fn download(&self) -> color_eyre::Result<Option<NeedSymlink>> {
        let bin_src = crate::downloaders::pip::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name()],
            self.bin_name(),
        )?;

        Ok(Some(NeedSymlink {
            src: bin_src.into(),
            dest: self.bin_dest_dir.clone().into(),
        }))
    }
}
