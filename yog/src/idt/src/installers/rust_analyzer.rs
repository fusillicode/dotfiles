use std::path::Path;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

/// Installer for rust-analyzer, the Rust language server.
///
/// rust-analyzer is the official language server for Rust, providing features
/// like code completion, diagnostics, and navigation for Rust development.
/// It integrates with editors like VS Code, Neovim, and others.
///
/// This installer downloads the nightly build of rust-analyzer for macOS ARM64
/// from the official Rust language repository.
pub struct RustAnalyzer<'a> {
    /// The directory where the rust-analyzer binary will be installed.
    pub bin_dir: &'a Path,
}

impl<'a> Installer for RustAnalyzer<'a> {
    fn bin_name(&self) -> &'static str {
        "rust-analyzer"
    }

    /// Downloads and installs the nightly build of rust-analyzer for macOS ARM64.
    ///
    /// This method performs the following steps:
    /// 1. Downloads the latest nightly build from the official rust-analyzer repository
    /// 2. Decompresses the gzipped binary directly to the bin directory
    /// 3. Makes the binary executable
    ///
    /// The nightly build is used because it contains the latest features and bug fixes,
    /// though it may be less stable than release builds.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the installation succeeds, or an error if any step fails.
    fn install(&self) -> color_eyre::Result<()> {
        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/rust-lang/{0}/releases/download/nightly/{0}-aarch64-apple-darwin.gz",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoZcat {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        utils::system::chmod_x(target)?;

        Ok(())
    }
}
