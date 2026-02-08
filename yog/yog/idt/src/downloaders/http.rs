use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use flate2::read::GzDecoder;
use rootcause::prelude::ResultExt as _;
use tar::Archive;
use xz2::read::XzDecoder;

pub enum HttpDeflateOption<'a> {
    DecompressGz {
        dest_path: &'a Path,
    },
    ExtractTarGz {
        dest_dir: &'a Path,
        // Option because not all the downloaded archives has a:
        // - stable name (i.e. `shellcheck`)
        // - a usable binary outside the archive (i.e. `elixir_ls` or `lua_ls`)
        // In these cases `dest_name` is set to None
        dest_name: Option<&'a str>,
    },
    ExtractTarXz {
        dest_dir: &'a Path,
        dest_name: Option<&'a str>,
    },
    ExtractZip {
        dest_dir: &'a Path,
        dest_name: Option<&'a str>,
    },
    WriteTo {
        dest_path: &'a Path,
    },
}

impl HttpDeflateOption<'_> {
    fn process(&self, tmp_file: &Path) -> rootcause::Result<PathBuf> {
        match self {
            Self::DecompressGz { dest_path } => {
                let input = File::open(tmp_file)
                    .context("error opening tmp file for gz decompression")
                    .attach_with(|| format!("path={}", tmp_file.display()))?;
                let mut decoder = GzDecoder::new(input);

                let mut dest = File::create(dest_path)
                    .context("error creating dest file")
                    .attach_with(|| format!("path={}", dest_path.display()))?;
                std::io::copy(&mut decoder, &mut dest)
                    .context("error decompressing gz to dest file")
                    .attach_with(|| format!("path={}", dest_path.display()))?;

                Ok(dest_path.into())
            }
            Self::ExtractTarGz { dest_dir, dest_name } => {
                let input = File::open(tmp_file)
                    .context("error opening tmp file for tar.gz extraction")
                    .attach_with(|| format!("path={}", tmp_file.display()))?;
                let decoder = GzDecoder::new(input);
                let archive = Archive::new(decoder);

                Ok(extract_tar(archive, tmp_file, dest_dir, *dest_name)?)
            }
            Self::ExtractTarXz { dest_dir, dest_name } => {
                let input = File::open(tmp_file)
                    .context("error opening tmp file for tar.xz extraction")
                    .attach_with(|| format!("path={}", tmp_file.display()))?;
                let decoder = XzDecoder::new(input);
                let archive = Archive::new(decoder);

                Ok(extract_tar(archive, tmp_file, dest_dir, *dest_name)?)
            }
            Self::ExtractZip { dest_dir, dest_name } => {
                let input = File::open(tmp_file)
                    .context("error opening tmp file for zip extraction")
                    .attach_with(|| format!("path={}", tmp_file.display()))?;
                let reader = BufReader::new(input);
                let mut archive = zip::ZipArchive::new(reader)
                    .context("error reading zip archive")
                    .attach_with(|| format!("path={}", tmp_file.display()))?;

                if let Some(dest_name) = dest_name {
                    let mut entry = archive
                        .by_name(dest_name)
                        .context("error finding entry in zip archive")
                        .attach_with(|| format!("path={}", tmp_file.display()))
                        .attach_with(|| format!("entry={dest_name}"))?;
                    let dest_path = dest_dir.join(dest_name);
                    let mut dest = File::create(&dest_path)
                        .context("error creating dest file for zip entry")
                        .attach_with(|| format!("path={}", dest_path.display()))?;
                    std::io::copy(&mut entry, &mut dest)
                        .context("error extracting zip entry")
                        .attach_with(|| format!("path={}", tmp_file.display()))
                        .attach_with(|| format!("entry={dest_name}"))?;

                    Ok(dest_path)
                } else {
                    archive
                        .extract(dest_dir)
                        .context("error extracting zip archive")
                        .attach_with(|| format!("path={}", tmp_file.display()))
                        .attach_with(|| format!("dest_dir={}", dest_dir.display()))?;

                    Ok(dest_dir.into())
                }
            }
            Self::WriteTo { dest_path } => {
                // Use copy instead of rename to handle cross-filesystem moves (e.g. /tmp -> target).
                std::fs::copy(tmp_file, dest_path)
                    .context("error copying tmp file to dest")
                    .attach_with(|| format!("src={}", tmp_file.display()))
                    .attach_with(|| format!("dest={}", dest_path.display()))?;

                Ok(dest_path.into())
            }
        }
    }
}

/// Source for verifying the checksum of a downloaded file.
pub struct ChecksumSource<'a> {
    /// URL to a checksums file (e.g., SHA256SUMS).
    pub checksums_url: &'a str,
    /// The filename to look up in the checksums file.
    pub filename: &'a str,
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

/// Extracts a tar archive to `dest_dir`. When `dest_name` is `Some`, only the matching entry is
/// extracted; otherwise the entire archive is unpacked.
fn extract_tar<R: std::io::Read>(
    mut archive: Archive<R>,
    archive_path: &Path,
    dest_dir: &Path,
    dest_name: Option<&str>,
) -> rootcause::Result<PathBuf> {
    if let Some(dest_name) = dest_name {
        for entry in archive
            .entries()
            .context("error reading tar entries")
            .attach_with(|| format!("path={}", archive_path.display()))?
        {
            let mut entry = entry
                .context("error reading tar entry")
                .attach_with(|| format!("entry={dest_name}"))?;
            let entry_path = entry
                .path()
                .context("error reading tar entry path")
                .attach_with(|| format!("entry={dest_name}"))?;
            if entry_path.to_str() == Some(dest_name) {
                let dest_path = dest_dir.join(dest_name);
                entry
                    .unpack(&dest_path)
                    .context("error extracting tar entry")
                    .attach_with(|| format!("entry={dest_name}"))?;
                return Ok(dest_path);
            }
        }
        Err(rootcause::report!("entry not found in tar archive")).attach_with(|| format!("entry={dest_name}"))
    } else {
        archive
            .unpack(dest_dir)
            .context("error extracting tar archive")
            .attach_with(|| format!("dest_dir={}", dest_dir.display()))?;
        Ok(dest_dir.into())
    }
}
