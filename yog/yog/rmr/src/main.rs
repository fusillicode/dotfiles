//! Remove files or directories passed as CLI args (recursive for dirs).
//!
//! Strips trailing `:...` suffix from paths before deletion.
//!
//! # Errors
//! - I/O errors from file/directory removal.

use std::fs::Metadata;
use std::path::Path;

use color_eyre::Report;
use color_eyre::eyre::bail;
use color_eyre::owo_colors::OwoColorize;
use ytil_sys::cli::Args;

/// Deletes one path after stripping the first ':' suffix segment.
///
/// # Errors
/// - Metadata retrieval or deletion fails.
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
                std::fs::remove_file(path).inspect_err(|err| {
                    eprintln!(
                        "Cannot delete file={} error={}",
                        path.display(),
                        format!("{err:?}").red()
                    );
                })?;
                return Ok(());
            }
            if ft.is_dir() {
                std::fs::remove_dir_all(path).inspect_err(|err| {
                    eprintln!(
                        "Cannot delete dir={} error={}",
                        path.display(),
                        format!("{err:?}").red()
                    );
                })?;
                return Ok(());
            }
            bail!("{}", format!("Not found path={}", path.display()).red())
        })
}

/// Strips suffix beginning at first ':'.
fn before_first_colon(s: &str) -> &str {
    s.split_once(':').map_or(s, |(before, _)| before)
}

/// Remove files or directories passed as CLI args (recursive for dirs).
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let files = ytil_sys::cli::get();

    if files.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

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
