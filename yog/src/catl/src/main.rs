#![feature(exit_status_error)]

use std::process::Command;

use anyhow::anyhow;

/// `cat` or `ls` based on what's supplied, i.e. a file or a directory.
fn main() -> anyhow::Result<()> {
    let args = utils::system::get_args();

    let path = args
        .first()
        .ok_or_else(|| anyhow!("missing path arg from {args:?}"))?;

    let metadata = std::fs::metadata(path)?;

    if metadata.is_dir() {
        return Ok(Command::new("ls")
            .args(["-llAtrh", path])
            .status()?
            .exit_ok()?);
    }

    if metadata.is_file() || metadata.is_symlink() {
        return Ok(Command::new("cat").args([path]).status()?.exit_ok()?);
    }

    Ok(())
}
