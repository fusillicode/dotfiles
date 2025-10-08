//! Display file contents or long‑list directories.
//!
//! # Arguments
//! - `<path>` Path to file / directory / symlink to display.
//!
//! # Usage
//! ```bash
//! catl <path> # file -> cat; directory -> colored long listing
//! ```
//!
//! # Errors
//! - Fetching metadata for `<path>` fails.
//! - Spawning or waiting on `cat` / `ls` fails.
//! - Underlying command exits with non-zero status.
#![feature(exit_status_error)]

use std::process::Command;

use color_eyre::eyre::eyre;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();

    let path = args.first().ok_or_else(|| eyre!("missing path arg | args={args:#?}"))?;

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
