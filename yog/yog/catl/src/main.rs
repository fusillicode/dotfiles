//! Display file contents or long-list directories.
//!
//! # Errors
//! - Metadata retrieval or command execution fails.
#![feature(exit_status_error)]

use std::process::Command;

use rootcause::prelude::ResultExt as _;
use rootcause::report;
use ytil_sys::cli::Args;

/// Display file contents or long‑list directories.
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let path = args
        .first()
        .ok_or_else(|| report!("missing path arg"))
        .attach_with(|| format!("args={args:#?}"))?;

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

    Err(report!("unsupported file type").attach(format!("path={path:?}")))
}
