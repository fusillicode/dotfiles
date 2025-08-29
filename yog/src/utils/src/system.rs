use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::thread::JoinHandle;

use color_eyre::eyre;
use color_eyre::eyre::OptionExt as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use color_eyre::owo_colors::OwoColorize as _;

/// Retrieves command-line arguments excluding the program name, returning them as a [`Vec`] of [`String`].
pub fn get_args() -> Vec<String> {
    let mut args = std::env::args();
    args.next();
    args.collect::<Vec<String>>()
}

/// Joins a thread handle and returns the result, handling join errors as [`eyre::Error`].
pub fn join<T>(join_handle: JoinHandle<color_eyre::Result<T>>) -> Result<T, eyre::Error> {
    join_handle.join().map_err(|error| eyre!("join error {error:#?}"))?
}

/// Builds a path starting from the home directory by appending the given parts, returning a [`PathBuf`].
pub fn build_home_path<P: AsRef<Path>>(parts: &[P]) -> color_eyre::Result<PathBuf> {
    let mut home_path = std::env::home_dir().ok_or_eyre("missing home dir")?;
    for part in parts {
        home_path.push(part);
    }
    Ok(home_path)
}

/// Copies the given content to the system clipboard using the `pbcopy` command.
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

/// Sets executable permissions (755) on the specified file path.
pub fn chmod_x<P: AsRef<Path>>(path: P) -> color_eyre::Result<()> {
    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms)?;
    Ok(())
}

/// Sets executable permissions on all files in the specified directory.
pub fn chmod_x_files_in_dir<P: AsRef<Path>>(dir: P) -> color_eyre::Result<()> {
    for target_res in std::fs::read_dir(dir)? {
        let target = target_res?.path();
        if target.is_file() {
            chmod_x(&target)?;
        }
    }
    Ok(())
}

/// Creates a symbolic link from the target to the link path, removing any existing file at the link location.
pub fn ln_sf<P: AsRef<Path>>(target: P, link: P) -> color_eyre::Result<()> {
    if link.as_ref().try_exists()? {
        std::fs::remove_file(&link)?;
    }
    std::os::unix::fs::symlink(target, &link)?;
    Ok(())
}

/// Creates symbolic links for all files in the target directory to the link directory.
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

/// Removes dead symbolic links from the specified directory.
pub fn rm_dead_symlinks(dir: &str) -> color_eyre::Result<()> {
    for entry_res in std::fs::read_dir(dir)? {
        let entry = entry_res?;
        let path = entry.path();

        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() && std::fs::metadata(&path).is_err() {
            std::fs::remove_file(&path)?;
            println!("{} {path:#?}", "Deleted dead symlink".cyan().bold());
        }
    }
    Ok(())
}

/// Removes the file at the specified path, ignoring if the file does not exist.
pub fn rm_f<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    std::fs::remove_file(path).or_else(|error| {
        if std::io::ErrorKind::NotFound == error.kind() {
            return Ok(());
        }
        Err(error)
    })
}
