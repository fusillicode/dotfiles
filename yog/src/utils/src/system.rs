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

pub fn home_path<P: AsRef<Path>>(path: P) -> color_eyre::Result<PathBuf> {
    Ok(PathBuf::from(&std::env::var("HOME")?).join(path))
}

pub fn cp_to_system_clipboard(content: &mut &[u8]) -> color_eyre::Result<()> {
    let mut pbcopy_child = crate::cmd::silent_cmd("pbcopy").stdin(Stdio::piped()).spawn()?;
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

pub fn chmod_x_files_in_dir<P: AsRef<Path>>(dir: P) -> color_eyre::Result<()> {
    for target in std::fs::read_dir(dir)? {
        let target = target?.path();
        if target.is_file() {
            chmod_x(&target)?;
        }
    }
    Ok(())
}

pub fn ln_sf<P: AsRef<Path>>(target: P, link: P) -> color_eyre::Result<()> {
    if link.as_ref().try_exists()? {
        std::fs::remove_file(&link)?;
    }
    std::os::unix::fs::symlink(target, &link)?;
    Ok(())
}

pub fn ln_sf_files_in_dir<P: AsRef<std::path::Path>>(target_dir: P, link_dir: P) -> color_eyre::Result<()> {
    for target in std::fs::read_dir(target_dir)? {
        let target = target?.path();
        if target.is_file() {
            let target_name = target
                .file_name()
                .ok_or_else(|| eyre!("target {target:?} without filename"))?;
            let link = link_dir.as_ref().join(target_name);
            ln_sf(target, link)?;
        }
    }
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
