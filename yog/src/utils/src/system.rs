use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::thread::JoinHandle;

use color_eyre::eyre;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

pub fn get_args() -> Vec<String> {
    let mut args = std::env::args();
    args.next();
    args.collect::<Vec<String>>()
}

pub fn join<T>(join_handle: JoinHandle<color_eyre::Result<T>>) -> Result<T, eyre::Error> {
    join_handle.join().map_err(|e| eyre!("join error {e:#?}"))?
}

pub fn cp_to_system_clipboard(content: &mut &[u8]) -> color_eyre::Result<()> {
    let mut pbcopy_child = crate::cmd::silent_cmd("pbcopy")
        .stdin(Stdio::piped())
        .spawn()?;
    std::io::copy(
        content,
        pbcopy_child
            .stdin
            .as_mut()
            .ok_or_else(|| eyre!("cannot get child stdin as mut"))?,
    )?;
    if !pbcopy_child.wait()?.success() {
        bail!("error copy content to system clipboard, content {content:#?}");
    }
    Ok(())
}

pub fn chmod_x<P: AsRef<Path>>(path: P) -> color_eyre::Result<()> {
    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms)?;
    Ok(())
}

/// Unix-only function to mimic `ln -sf` with simple glob support (only '*' at the end)
pub fn ln_sf(src: &Path, dest: &Path) -> color_eyre::Result<()> {
    fn get_path_entries(path: &Path) -> color_eyre::Result<Vec<PathBuf>> {
        if path.file_name().is_some_and(|p| p == "*")
            && let Some(parent) = path.parent()
        {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(parent)? {
                let entry = entry?;
                entries.push(entry.path());
            }
            return Ok(entries);
        }
        if !path.exists() {
            bail!("Path {path:?} does not exists")
        }
        Ok(vec![path.to_path_buf()])
    }

    let _src_entries = get_path_entries(src)?;

    if dest.exists() || dest.is_symlink() {
        std::fs::remove_file(dest)?;
    }
    std::os::unix::fs::symlink(src, dest)?;
    Ok(())
}

pub fn rm_dead_symlinks(dir: &str) -> color_eyre::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() && std::fs::metadata(&path).is_err() {
            println!("🗑️ Removing dead symlink: {path:#?}");
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

pub fn rm_f<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    std::fs::remove_file(path).or_else(|error| {
        if std::io::ErrorKind::NotFound == error.kind() {
            return Ok(());
        }
        Err(error)
    })
}
