use std::process::Command;

use crate::ToolInstaller;
use crate::downloaders::curl::OutputOption;

// For Markdown preview with peek.nvim
pub struct Deno {
    pub bin_dest_dir: String,
}

impl ToolInstaller for Deno {
    fn bin_name(&self) -> &'static str {
        "deno"
    }

    fn download(&self) -> color_eyre::Result<()> {
        let repo = format!("{0}land/{0}", self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-aarch64-apple-darwin.zip",
                self.bin_name()
            ),
            OutputOption::PipeInto(
                Command::new("tar").args(["-xz", "-C"]),
                self.bin_dest_dir.clone(),
            ),
        )?;

        Ok(())
    }
}
