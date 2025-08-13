use utils::system::symlink::Symlink;

use crate::Installer;

pub struct PrettierD {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for PrettierD {
    fn bin_name(&self) -> &'static str {
        "prettierd"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[&format!("@fsouza/{}", self.bin_name())],
        )?;

        let link = format!("{}/{}", self.bin_dir, self.bin_name());
        let symlink = utils::system::symlink::build(&target_dir, Some(&link))?;

        Ok(symlink)
    }
}
