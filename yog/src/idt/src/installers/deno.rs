use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

// For Markdown preview with peek.nvim
pub struct Deno {
    pub bin_dir: String,
}

impl Installer for Deno {
    fn bin_name(&self) -> &'static str {
        "deno"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("{0}land/{0}", self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-aarch64-apple-darwin.zip",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: &self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        Ok(())
    }
}
