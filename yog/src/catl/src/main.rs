#![feature(exit_status_error)]

use std::process::Command;

use color_eyre::eyre::eyre;

/// A utility that displays file contents or directory listings based on the input path.
///
/// This tool automatically determines whether the provided path is a file or directory:
/// - For files: displays the content using `cat`
/// - For directories: lists contents using `ls -llAtrh`
/// - For symlinks: treats as files and displays content using `cat`
///
/// # Arguments
///
/// * `path` - The file or directory path to display
///
/// # Examples
///
/// Display a file:
/// ```bash
/// catl /path/to/file.txt
/// ```
///
/// List a directory:
/// ```bash
/// catl /path/to/directory
/// ```
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = utils::system::get_args();

    let path = args.first().ok_or_else(|| eyre!("missing path arg from {args:#?}"))?;

    let metadata = std::fs::metadata(path)?;

    if metadata.is_dir() {
        return Ok(Command::new("ls").args(["-llAtrh", path]).status()?.exit_ok()?);
    }

    if metadata.is_file() || metadata.is_symlink() {
        return Ok(Command::new("cat").args([path]).status()?.exit_ok()?);
    }

    Ok(())
}
