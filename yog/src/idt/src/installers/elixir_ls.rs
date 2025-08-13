use utils::system::symlink::Symlink;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct ElixirLs {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for ElixirLs {
    fn bin_name(&self) -> &'static str {
        "elixir-ls"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let repo = format!("elixir-lsp/{}", self.bin_name());
        let dev_tools_repo_dir = format!("{}/{}", self.dev_tools_dir, self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.zip",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: &dev_tools_repo_dir,
                dest_name: None,
            },
        )?;

        let link = format!("{}/{}", self.bin_dir, self.bin_name());
        let symlink = utils::system::symlink::build(
            &format!("{dev_tools_repo_dir}/language_server.sh"),
            Some(&link),
        )?;

        Ok(symlink)
    }
}
