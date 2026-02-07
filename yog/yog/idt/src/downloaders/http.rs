use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use rootcause::prelude::ResultExt as _;

pub enum HttpDownloaderOption<'a> {
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
    WriteTo {
        dest_path: &'a Path,
    },
}

/// Source for verifying the checksum of a downloaded file.
pub struct ChecksumSource<'a> {
    /// URL to a checksums file (e.g., SHA256SUMS).
    pub checksums_url: &'a str,
    /// The filename to look up in the checksums file.
    pub filename: &'a str,
}

/// Downloads a file from the given URL with the specified [`HttpDownloaderOption`].
///
/// The file is always downloaded to a temporary location first. If a [`ChecksumSource`] is provided,
/// the SHA256 checksum is verified before processing. If `checksum` is `None`, processing proceeds
/// without verification.
///
/// # Errors
/// - The HTTP request fails or returns a non-success status.
/// - Executing a decompression command (`gzip`, `tar`) fails or returns a non-zero exit status.
/// - A filesystem operation (create/read/write/remove) fails.
/// - Checksum verification fails (mismatch).
/// - Creating a temporary directory fails.
pub fn run(url: &str, opt: &HttpDownloaderOption, checksum: Option<&ChecksumSource>) -> rootcause::Result<PathBuf> {
    // Phase 1: Download to a temporary file.
    let tmp_dir = tempfile::tempdir().context("error creating temp dir for download")?;
    let tmp_file = tmp_dir.path().join("download");

    let resp = ureq::get(url)
        .call()
        .context("error downloading")
        .attach_with(|| format!("url={url}"))?;

    let mut file = File::create(&tmp_file).context("error creating temp file")?;
    std::io::copy(&mut resp.into_body().as_reader(), &mut file)
        .context("error writing download to temp file")
        .attach_with(|| format!("url={url}"))?;

    // Phase 2: Checksum verification (only when a source is provided).
    if let Some(source) = checksum {
        let expected = crate::downloaders::checksum::download_and_find_checksum(source.checksums_url, source.filename)?;
        crate::downloaders::checksum::verify(&tmp_file, &expected)?;
    }

    // Phase 3: Process the downloaded file according to the option.
    let target = match opt {
        HttpDownloaderOption::DecompressGz { dest_path } => {
            // Use `gzip -dc` because macOS `zcat` expects `.Z` files, not `.gz`.
            let output = Command::new("gzip")
                .args(["-dc"])
                .arg(&tmp_file)
                .output()
                .context("error executing gzip -dc")?;
            output.status.exit_ok().context("error gzip -dc exit status")?;

            let mut file = File::create(dest_path)
                .context("error creating dest file")
                .attach_with(|| format!("path={}", dest_path.display()))?;
            file.write_all(&output.stdout)
                .context("error writing dest file")
                .attach_with(|| format!("path={}", dest_path.display()))?;

            dest_path.into()
        }
        HttpDownloaderOption::ExtractTarGz { dest_dir, dest_name } => {
            let mut tar_cmd = Command::new("tar");
            tar_cmd.args([
                "-xz",
                "-C",
                &dest_dir.to_string_lossy(),
                "-f",
                &tmp_file.to_string_lossy(),
            ]);
            if let Some(dest_name) = dest_name {
                tar_cmd.arg(dest_name);
            }
            tar_cmd
                .status()
                .context("error executing tar")?
                .exit_ok()
                .context("error tar exit status")?;

            dest_name.map_or_else(|| dest_dir.into(), |dn| dest_dir.join(dn))
        }
        HttpDownloaderOption::WriteTo { dest_path } => {
            // Use copy instead of rename to handle cross-filesystem moves (e.g. /tmp -> target).
            std::fs::copy(&tmp_file, dest_path)
                .context("error copying temp file to dest")
                .attach_with(|| format!("src={} dest={}", tmp_file.display(), dest_path.display()))?;

            dest_path.into()
        }
    };

    Ok(target)
}
