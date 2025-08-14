use utils::system::symlink::Symlink;
use utils::system::symlink::SymlinkFile;

use crate::Installer;

pub struct GraphQlLsp {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for GraphQlLsp {
    fn bin_name(&self) -> &'static str {
        "graphql-lsp"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            "graphql-language-service-cli",
            &["graphql-language-service-cli"],
        )?;

        let symlink = SymlinkFile::new(
            &format!("{target_dir}/{}", self.bin_name()),
            &format!("{}/{}", self.bin_dir, self.bin_name()),
        )?;
        Ok(Box::new(symlink))
    }
}
