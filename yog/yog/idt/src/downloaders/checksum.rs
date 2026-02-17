use std::fs::File;
use std::io::Read;
use std::path::Path;

use rootcause::prelude::ResultExt as _;
use rootcause::report;
use sha2::Digest as _;

/// Computes the SHA256 hex digest of the file at `path`.
///
/// # Errors
/// - The file cannot be opened or read.
pub fn compute_sha256(path: &Path) -> rootcause::Result<String> {
    let mut file = File::open(path)
        .context("error opening file for checksum")
        .attach_with(|| format!("path={}", path.display()))?;

    let mut hasher = sha2::Sha256::new();
    let mut buf = [0_u8; 8192];

    loop {
        let n = file
            .read(&mut buf)
            .context("error reading file for checksum")
            .attach_with(|| format!("path={}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(
            buf.get(..n)
                .ok_or_else(|| report!("error slicing buffer"))
                .attach_with(|| format!("n={n} buf_len={}", buf.len()))?,
        );
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Downloads a checksums file from `checksums_url` and extracts the expected hash for `filename`.
///
/// Supports the standard `<hex_hash>  <filename>` format used by `sha256sum` / `shasum`, as well as
/// single-hash files (one line containing only a hex hash).
///
/// # Errors
/// - The checksums file download fails.
/// - The file cannot be read as UTF-8.
/// - No matching entry is found for `filename`.
pub fn download_and_find_checksum(checksums_url: &str, filename: &str) -> rootcause::Result<String> {
    let body = ureq::get(checksums_url)
        .call()
        .context("error downloading checksums file")
        .attach_with(|| format!("url={checksums_url}"))?
        .into_body()
        .read_to_string()
        .context("error reading checksums response")
        .attach_with(|| format!("url={checksums_url}"))?;

    parse_checksum(&body, filename)
}

/// Parse a checksums file content and find the hash for `filename`.
///
/// Handles two formats:
/// 1. Multi-line: `<hex_hash>  <filename>` (with one or two spaces)
/// 2. Single-line: just a hex hash (for per-file `.sha256` files)
///
/// # Errors
/// - No matching entry found for `filename`.
fn parse_checksum(content: &str, filename: &str) -> rootcause::Result<String> {
    let trimmed = content.trim();

    // Single-line file containing only a hex hash (per-file .sha256 pattern).
    if !trimmed.contains(' ') && !trimmed.contains('\n') && !trimmed.is_empty() {
        return Ok(trimmed.to_owned());
    }

    // Standard multi-line format: `<hash>  <filename>` or `<hash> <filename>`
    for line in trimmed.lines() {
        let line = line.trim();
        // Split at first whitespace
        if let Some((hash, rest)) = line.split_once(' ') {
            let rest = rest.trim_start();
            // The filename in checksums files may have a leading `*` (binary mode indicator)
            let entry_filename = rest.strip_prefix('*').unwrap_or(rest);
            if entry_filename == filename {
                return Ok(hash.to_owned());
            }
        }
    }

    Err(report!("error checksum entry not found").attach(format!("filename={filename:?} content={trimmed:?}")))
}

/// Verifies that the file at `path` matches the `expected_hex` SHA256 hash.
///
/// # Errors
/// - Computing the hash fails.
/// - The computed hash does not match.
pub fn verify(path: &Path, expected_hex: &str) -> rootcause::Result<()> {
    let actual = compute_sha256(path)?;
    let expected = expected_hex.to_lowercase();

    if actual != expected {
        return Err(report!("error checksum mismatch")
            .attach(format!("path={} expected={expected} actual={actual}", path.display())));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::multi_line_format("abc123  foo.tar.gz\ndef456  bar.zip\n", "bar.zip", "def456")]
    #[case::single_space("abc123 foo.tar.gz\n", "foo.tar.gz", "abc123")]
    #[case::binary_mode_indicator("abc123 *foo.tar.gz\n", "foo.tar.gz", "abc123")]
    #[case::single_line_hash("abc123def456\n", "anything", "abc123def456")]
    fn parse_checksum_returns_expected_hash(#[case] content: &str, #[case] filename: &str, #[case] expected: &str) {
        assert2::assert!(let Ok(hash) = parse_checksum(content, filename));
        pretty_assertions::assert_eq!(hash, expected);
    }

    #[test]
    fn parse_checksum_returns_error_when_not_found() {
        let content = "abc123  foo.tar.gz\ndef456  bar.zip\n";
        assert2::assert!(let Err(err) = parse_checksum(content, "missing.txt"));
        assert!(err.to_string().contains("error checksum entry not found"));
    }

    #[test]
    fn compute_sha256_returns_expected_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, b"hello world").unwrap();

        assert2::assert!(let Ok(hash) = compute_sha256(&file_path));
        pretty_assertions::assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn verify_succeeds_with_matching_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, b"hello world").unwrap();

        assert2::assert!(let
            Ok(()) = verify(
                &file_path,
                "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
            )
        );
    }

    #[test]
    fn verify_fails_with_mismatched_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, b"hello world").unwrap();

        assert2::assert!(let
            Err(err) = verify(
                &file_path,
                "0000000000000000000000000000000000000000000000000000000000000000"
            )
        );
        assert!(err.to_string().contains("error checksum mismatch"));
    }
}
