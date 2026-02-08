use std::fs::File;
use std::path::PathBuf;

use rootcause::prelude::ResultExt as _;

pub mod deflate;
pub mod github;

use deflate::ChecksumSource;
use deflate::HttpDeflateOption;

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
) -> rootcause::Result<PathBuf> {
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
    if let Some(source) = checksum {
        let expected = crate::downloaders::checksum::download_and_find_checksum(source.checksums_url, source.filename)?;
        crate::downloaders::checksum::verify(&tmp_file, &expected)?;
    }

    // Phase 3: Process the downloaded file according to the option.
    deflate_opt.process(&tmp_file)
}
