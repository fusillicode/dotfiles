use utils::system::symlink::Symlink;

use crate::Installer;

pub struct SqlLanguageServer {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for SqlLanguageServer {
    fn bin_name(&self) -> &'static str {
        "sql-language-server"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target_dir =
            crate::downloaders::npm::run(&self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        let target = format!("{target_dir}/{}", self.bin_name());
        let symlink = utils::system::symlink::build(&target, Some(&self.bin_dir))?;

        Ok(symlink)
    }
}
