use utils::system::symlink::SymlinkFile;
use utils::system::symlink::SymlinkOp;

use crate::Installer;

pub struct Commitlint {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for Commitlint {
    fn bin_name(&self) -> &'static str {
        "commitlint"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn SymlinkOp>> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[
                &format!("@{}/cli", self.bin_name()),
                &format!("@{}/config-conventional", self.bin_name()),
            ],
        )?;

        let symlink = SymlinkFile::new(
            &format!("{target_dir}/{}", self.bin_name()),
            &format!("{}/{}", self.bin_dir, self.bin_name()),
        )?;
        Ok(Box::new(symlink))
    }
}
