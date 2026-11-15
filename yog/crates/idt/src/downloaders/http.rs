use std::fs::File;
use std::path::PathBuf;

use deflate::ChecksumSource;
use deflate::HttpDeflateOption;
use rootcause::prelude::ResultExt;

use crate::downloaders::checksum;
use crate::downloaders::checksum::ChecksumOutcome;

pub mod deflate;
pub mod github;

/// A processed artifact and the checksum outcome produced by its download.
pub struct DownloadedArtifact {
    pub path: PathBuf,
    checksum: ChecksumOutcome,
}

impl DownloadedArtifact {
    pub const fn checksum(&self) -> ChecksumOutcome {
        self.checksum
    }
}

/// Downloads a file from the given URL with the specified [`HttpDeflateOption`].
///
/// The file is always downloaded to a temporary location first. If a [`ChecksumSource`] is provided,
/// the SHA256 checksum is verified before processing. If `checksum` is `None`, processing proceeds
/// without verification.
///
/// # Errors
/// - The HTTP request fails or returns a non-success status.
/// - Decompression or archive extraction fails.
/// - A filesystem operation (create/read/write/remove) fails.
/// - Checksum verification fails (mismatch).
/// - Creating a temporary directory fails.
pub fn run(
    url: &str,
    deflate_opt: &HttpDeflateOption,
    checksum: Option<&ChecksumSource>,
) -> rootcause::Result<DownloadedArtifact> {
    // Phase 1: Download to a temporary file.
    let tmp_dir = tempfile::tempdir().context("error creating tmp dir for download")?;
    let tmp_file = tmp_dir.path().join("download");

    let resp = ureq::get(url)
        .call()
        .context("error downloading")
        .attach_with(|| format!("url={url}"))?;

    let mut file = File::create(&tmp_file).context("error creating tmp file")?;
    std::io::copy(&mut resp.into_body().as_reader(), &mut file)
        .context("error writing download to tmp file")
        .attach_with(|| format!("url={url}"))?;

    // Phase 2: Checksum verification (only when a source is provided).
    let checksum = checksum::check_download(&tmp_file, checksum)?;

    // Phase 3: Process the downloaded file according to the option.
    let path = deflate_opt.process(&tmp_file)?;

    Ok(DownloadedArtifact { path, checksum })
}
