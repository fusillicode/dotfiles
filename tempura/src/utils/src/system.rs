use std::process::Command;
use std::process::Stdio;
use std::thread::JoinHandle;

use anyhow::anyhow;
use anyhow::bail;

pub fn join<T>(join_handle: JoinHandle<anyhow::Result<T>>) -> Result<T, anyhow::Error> {
    join_handle
        .join()
        .map_err(|e| anyhow!("join error {e:?}"))?
}

pub fn copy_to_system_clipboard(content: &mut &[u8]) -> anyhow::Result<()> {
    let mut pbcopy_child = silent_cmd("pbcopy").stdin(Stdio::piped()).spawn()?;
    std::io::copy(
        content,
        pbcopy_child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("cannot get child stdin as mut"))?,
    )?;
    if !pbcopy_child.wait()?.success() {
        bail!("error copy content to system clipboard, content {content:?}");
    }
    Ok(())
}

// Yes, `dir` is a `&str` and it's not sanitized but...I'm the alpha & the omega here!
pub fn chmod_x(dir: &str) -> anyhow::Result<()> {
    Ok(silent_cmd("sh")
        .args(["-c", &format!("chmod +x {dir}")])
        .status()?
        .exit_ok()?)
}

pub fn silent_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    if !cfg!(debug_assertions) {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    cmd
}

pub fn rm_dead_symlinks(dir: &str) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() && std::fs::metadata(&path).is_err() {
            println!("Removing dead symlink: {path:?}");
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}
