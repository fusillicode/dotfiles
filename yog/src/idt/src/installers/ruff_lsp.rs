use utils::system::symlink::Symlink;

use crate::Installer;

pub struct RuffLsp {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for RuffLsp {
    fn bin_name(&self) -> &'static str {
        "ruff-lsp"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target_dir =
            crate::downloaders::pip::run(&self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        let target = format!("{target_dir}/{}", self.bin_name());
        let symlink = utils::system::symlink::build(&target, Some(&self.bin_dir))?;

        Ok(symlink)
    }
}
