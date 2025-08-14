use utils::system::symlink::SymlinkFile;
use utils::system::symlink::SymlinkOp;

use crate::Installer;

pub struct RuffLsp {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for RuffLsp {
    fn bin_name(&self) -> &'static str {
        "ruff-lsp"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn SymlinkOp>> {
        let target_dir =
            crate::downloaders::pip::run(&self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        let symlink = SymlinkFile::new(
            &format!("{target_dir}/{}", self.bin_name()),
            &format!("{}/{}", self.bin_dir, self.bin_name()),
        )?;
        Ok(Box::new(symlink))
    }
}
