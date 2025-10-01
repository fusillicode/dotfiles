use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct ElixirLs<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for ElixirLs<'_> {
    fn bin_name(&self) -> &'static str {
        "elixir-ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let dev_tools_repo_dir = self.dev_tools_dir.join(self.bin_name());
        let repo = format!("elixir-lsp/{}", self.bin_name());
        let latest_release = ytil_github::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.zip",
                self.bin_name()
            ),
            &CurlDownloaderOption::PipeIntoTar {
                dest_dir: &dev_tools_repo_dir,
                dest_name: None,
            },
        )?;

        ytil_system::ln_sf(
            &dev_tools_repo_dir.join("language_server.sh"),
            &self.bin_dir.join(self.bin_name()),
        )?;

        ytil_system::chmod_x_files_in_dir(&dev_tools_repo_dir)?;

        Ok(())
    }

    // NOTE: skip because hopefully soon I'll not need this anymore...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }
}
