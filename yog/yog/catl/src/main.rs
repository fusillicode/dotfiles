//! Print a file with `cat` or long‑list a directory with `ls`.
#![feature(exit_status_error)]

use std::process::Command;

use color_eyre::eyre::eyre;

/// Display file contents or list a directory (long format).
/// Uses `cat` for files/symlinks, `ls -llAtrh` for directories.
///
/// # Usage
///
/// ```bash
/// catl <path>     # file -> cat; directory -> coloured long listing
/// ```
///
/// # Arguments
///
/// - `<path>` Path to file / directory / symlink to display.
///
/// # Errors
/// In case:
/// - Executing one of the external commands (cat, ls) fails or returns a non-zero exit status.
/// - A filesystem operation (open/read/write/remove) fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();

    let path = args.first().ok_or_else(|| eyre!("missing path arg from {args:#?}"))?;

    let metadata = std::fs::metadata(path)?;

    if metadata.is_dir() {
        return Ok(Command::new("ls")
            .args(["-llAtrh", "--color=always", path])
            .status()?
            .exit_ok()?);
    }

    if metadata.is_file() || metadata.is_symlink() {
        return Ok(Command::new("cat").args([path]).status()?.exit_ok()?);
    }

    Ok(())
}
