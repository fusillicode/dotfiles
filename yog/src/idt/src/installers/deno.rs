use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

/// Installer for Deno, a JavaScript/TypeScript runtime and tool.
///
/// Deno is used for various development tasks including running scripts,
/// bundling code, and providing a development server. It's particularly
/// useful for Markdown preview functionality with tools like peek.nvim.
///
/// This installer downloads the latest release from the official Deno repository
/// and installs it to the specified binary directory.
pub struct Deno<'a> {
    /// The directory where the deno binary will be installed.
    pub bin_dir: &'a Path,
}

impl<'a> Installer for Deno<'a> {
    fn bin_name(&self) -> &'static str {
        "deno"
    }

    /// Downloads and installs the latest Deno release for macOS ARM64.
    ///
    /// This method performs the following steps:
    /// 1. Fetches the latest release version from the Deno GitHub repository
    /// 2. Downloads the ARM64 macOS binary from GitHub releases
    /// 3. Extracts the binary to the configured bin directory
    /// 4. Makes the binary executable
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the installation succeeds, or an error if any step fails.
    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("{0}land/{0}", self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-aarch64-apple-darwin.zip",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        utils::system::chmod_x(&target)?;

        Ok(())
    }
}
