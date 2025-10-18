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

use std::path::Path;

use color_eyre::owo_colors::OwoColorize;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let files = ytil_system::get_args();

    let mut errors = false;
    for file in &files {
        let trimmed = before_first_colon(file);
        let path = Path::new(&trimmed);

        match path.symlink_metadata() {
            Ok(metadata) => {
                let ft = metadata.file_type();
                if ft.is_file() || ft.is_symlink() {
                    if let Err(error) = std::fs::remove_file(path) {
                        errors = true;
                        eprintln!(
                            "Cannot delete file={} error={}",
                            path.display(),
                            format!("{error:?}").red()
                        );
                    }
                    continue;
                }
                if ft.is_dir() {
                    if let Err(error) = std::fs::remove_dir_all(path) {
                        errors = true;
                        eprintln!(
                            "Cannot delete dir={} error={}",
                            path.display(),
                            format!("{error:?}").red()
                        );
                    }
                    continue;
                }
                errors = true;
                eprintln!("{}", format!("Not found path={}", path.display()).red());
            }
            Err(error) => {
                errors = true;
                eprintln!(
                    "Cannot read metadata of path={} error={}",
                    path.display(),
                    format!("{error:?}").red()
                );
            }
        }
    }

    if errors {
        std::process::exit(1);
    }

    Ok(())
}

/// Returns a subslice of `s` up to (excluding) the first ':'; strips everything from the first colon onward.
///
/// Performs a forward byte scan (ASCII match) without allocation.
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
pub fn before_first_colon(s: &str) -> &str {
    for (i, &b) in s.as_bytes().iter().enumerate() {
        if b == b':' {
            return &s[..i];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("hello", "hello")]
    #[case("alpha:", "alpha")]
    #[case("one:two:three", "one")]
    #[case(":rest", "")]
    #[case(":", "")]
    #[case("", "")]
    #[case("\u{03C0}:\u{03C4}:\u{03C9}", "\u{03C0}")]
    fn before_first_colon_when_various_inputs_strips_after_first_colon_returns_expected(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        pretty_assertions::assert_eq!(before_first_colon(input), expected);
    }
}
