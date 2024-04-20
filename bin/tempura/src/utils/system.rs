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
    let mut pbcopy_child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
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
    Ok(Command::new("sh")
        .args(["-c", &format!("chmod +x {dir}")])
        .status()?
        .exit_ok()?)
}
