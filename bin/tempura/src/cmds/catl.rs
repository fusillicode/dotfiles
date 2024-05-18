use std::fmt::Debug;

use anyhow::anyhow;

use crate::utils::system::silent_cmd;

pub fn run<'a>(mut args: impl Iterator<Item = &'a str> + Debug) -> anyhow::Result<()> {
    let path = args
        .next()
        .ok_or_else(|| anyhow!("missing path arg from {args:?}"))?;

    let metadata = std::fs::metadata(path)?;
    if metadata.is_dir() {
        silent_cmd("sh")
            .args(["-c", &format!("ls -llAtrh {}", path)])
            .status()?
            .exit_ok()?;
    } else if metadata.is_file() || metadata.is_symlink() {
        silent_cmd("sh")
            .args(["-c", &format!("cat {}", path)])
            .status()?
            .exit_ok()?;
    }

    Ok(())
}
