use std::fmt::Debug;

use anyhow::anyhow;

use crate::utils::system::silent_cmd;

pub fn run<'a>(mut args: impl Iterator<Item = &'a str> + Debug) -> anyhow::Result<()> {
    let path = args
        .next()
        .ok_or_else(|| anyhow!("missing path arg from {args:?}"))?;

    let metadata = std::fs::metadata(path)?;

    if metadata.is_dir() {
        return Ok(silent_cmd("ls")
            .args(["-llAtrh", path])
            .status()?
            .exit_ok()?);
    }

    if metadata.is_file() || metadata.is_symlink() {
        return Ok(silent_cmd("cat").args([path]).status()?.exit_ok()?);
    }

    Ok(())
}
