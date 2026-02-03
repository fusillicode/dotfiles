//! Display file contents or long-list directories.
//!
//! # Errors
//! - Metadata retrieval or command execution fails.
#![feature(exit_status_error)]

use std::process::Command;

use color_eyre::eyre::eyre;
use ytil_sys::cli::Args;

/// Display file contents or long‑list directories.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

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
