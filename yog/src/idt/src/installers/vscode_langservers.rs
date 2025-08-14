use utils::system::symlink::SymlinkFilesIntoDir;
use utils::system::symlink::SymlinkOp;

use crate::Installer;

pub struct VsCodeLangServers {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for VsCodeLangServers {
    fn bin_name(&self) -> &'static str {
        "vscode-langservers-extracted"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn SymlinkOp>> {
        let target_dir =
            crate::downloaders::npm::run(&self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        let symlink = SymlinkFilesIntoDir::new(&format!("{target_dir}/*"), &self.bin_dir)?;
        Ok(Box::new(symlink))
    }
}
