//! Remove files or directories passed as CLI args (recursive for dirs).
//!
//! Strips trailing metadata suffix beginning at the first ':' in each argument (colon and suffix removed) before
//! deletion. Useful when piping annotated paths (e.g. from linters or search tools emitting `path:line:col`).
//!
//! # Arguments
//! - `<paths...>` One or more filesystem paths (files, symlinks, or directories). Optional trailing `:...` suffix is
//!   removed.
//!
//! # Returns
//! - Exit code 0 if all provided paths were deleted successfully (or no paths given).
//! - Exit code 1 if any path failed to delete or did not exist.
//!
//! # Errors
//! - Initialization failure from [`color_eyre::install`].
//! - I/O errors from [`std::fs::remove_file`] or [`std::fs::remove_dir_all`]. These are reported individually and
//!   contribute to a non-zero exit code.
//!
//! # Rationale
//! - Eliminates need for ad-hoc shell loops to mass-delete mixed file & directory sets while handling `tool:line:col`
//!   style suffixes.
//! - Colorized error reporting highlights problematic paths quickly.
//!
//! # Performance
//! - One reverse byte scan per argument to locate last ':' (no allocation).
//! - Single `symlink_metadata` call per path (branches on [`std::fs::FileType`]), minimizing metadata syscalls.
//! - Sequential deletions avoid contention; for huge argument lists, parallelism could help but increases complexity
//!   (ordering, error aggregation).
//!
//! # Future Work
//! - Add `--dry-run` flag for previewing deletions.
//! - Add parallel deletion (configurable) for large batches.
//! - Accept glob patterns expanded internally (on platforms without shell globbing).

use std::fs::Metadata;
use std::path::Path;

use color_eyre::Report;
use color_eyre::eyre::bail;
use color_eyre::owo_colors::OwoColorize;

/// Deletes one path after stripping the first ':' suffix segment.
///
/// Performs metadata lookup, branches on filetype, and deletes a file, symlink,
/// or directory. Emits colored error messages to stderr; caller aggregates
/// failures.
///
/// # Arguments
/// - `file` Raw CLI argument possibly containing suffixes like `:line:col`.
///
/// # Returns
/// - `Ok(())` if the path existed and was deleted (file, symlink, or directory).
/// - `Err` if metadata lookup fails, deletion fails, or the path type is unsupported / missing.
///
/// # Errors
/// - Metadata retrieval failure (permissions, not found, etc.).
/// - Deletion failure (I/O error removing file or directory).
/// - Unsupported path type (reported as "Not found").
///
/// # Performance
/// - Single metadata syscall plus one deletion syscall on success.
/// - No heap allocation besides error formatting.
///
/// # Future Work
/// - Distinguish success via a dedicated return type (e.g. `Result<Deleted, DeleteError>`).
fn process(file: &str) -> color_eyre::Result<()> {
    let trimmed = before_first_colon(file);
    let path = Path::new(&trimmed);

    path.symlink_metadata()
        .map_err(|error| {
            eprintln!(
                "Cannot read metadata of path={} error={}",
                path.display(),
                format!("{error:?}").red()
            );
            Report::from(error)
        })
        .and_then(|metadata: Metadata| -> color_eyre::Result<()> {
            let ft = metadata.file_type();
            if ft.is_file() || ft.is_symlink() {
                std::fs::remove_file(path).inspect_err(|error| {
                    eprintln!(
                        "Cannot delete file={} error={}",
                        path.display(),
                        format!("{error:?}").red()
                    );
                })?;
                return Ok(());
            }
            if ft.is_dir() {
                std::fs::remove_dir_all(path).inspect_err(|error| {
                    eprintln!(
                        "Cannot delete dir={} error={}",
                        path.display(),
                        format!("{error:?}").red()
                    );
                })?;
                return Ok(());
            }
            bail!("{}", format!("Not found path={}", path.display()).red())
        })
}

/// Strips suffix beginning at first ':'; returns subslice before colon.
///
/// # Arguments
/// - `s` Raw argument string potentially containing a suffix like `:line:col`.
///
/// # Returns
/// - Slice before the first ':'; original `s` if no ':' present.
///
/// # Performance
/// - Single forward traversal `O(n)`; avoids UTF-8 decoding (colon is ASCII).
/// - Simple explicit loop similar in cost to `find(':')`.
fn before_first_colon(s: &str) -> &str {
    for (i, &b) in s.as_bytes().iter().enumerate() {
        if b == b':' {
            return &s[..i];
        }
    }
    s
}

/// Remove files or directories passed as CLI args (recursive for dirs).
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let files = ytil_system::get_args();

    if files.is_empty() {
        println!("Nothing done");
    }

    let mut any_errors = false;
    for file in &files {
        if process(file).is_err() {
            any_errors = true;
        }
    }

    if any_errors {
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::no_colon("hello", "hello")]
    #[case::single_colon_at_end("alpha:", "alpha")]
    #[case::multiple_colons("one:two:three", "one")]
    #[case::colon_at_start(":rest", "")]
    #[case::only_colon(":", "")]
    #[case::empty_string("", "")]
    #[case::unicode_characters("\u{03C0}:\u{03C4}:\u{03C9}", "\u{03C0}")]
    fn before_first_colon_when_various_inputs_strips_after_first_colon_returns_expected(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        pretty_assertions::assert_eq!(before_first_colon(input), expected);
    }
}
