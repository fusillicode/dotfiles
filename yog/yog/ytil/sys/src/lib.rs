//! Provide cohesive system helpers: args, paths, symlinks, permissions, atomic copy, clipboard.
//!
//! Offer small utilities for CLI tools: joining thread handles, building home-relative paths,
//! manipulating filesystem entries (chmod, symlinks, atomic copy) and clipboard integration.

#![feature(exit_status_error)]

use std::collections::VecDeque;
use std::fs::DirEntry;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::thread::JoinHandle;

use chrono::Utc;
use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::OptionExt as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
pub use pico_args;

pub mod cli_args;
pub mod lsof;
pub mod rm;

/// Joins a thread handle and returns the result, handling join errors as [`eyre::Error`].
/// Awaits a `JoinHandle` and unwraps the inner `Result`.
///
/// # Errors
/// - The task panicked.
/// - The task returned an error.
pub fn join<T>(join_handle: JoinHandle<color_eyre::Result<T>>) -> Result<T, eyre::Error> {
    join_handle
        .join()
        .map_err(|err| eyre!("error joining handle | error={err:#?}"))?
}

/// Builds a path starting from the home directory by appending the given parts, returning a [`PathBuf`].
///
/// # Errors
/// - The home directory cannot be determined.
pub fn build_home_path<P: AsRef<Path>>(parts: &[P]) -> color_eyre::Result<PathBuf> {
    let home_path = std::env::home_dir().ok_or_eyre("missing home dir | env=HOME")?;
    Ok(build_path(home_path, parts))
}

/// Builds a path by appending multiple parts to a root path.
///
/// # Arguments
/// - `root` The base path to start with.
/// - `parts` A slice of path components to append.
///
/// # Returns
/// A new [`PathBuf`] with all parts appended to the root.
pub fn build_path<P: AsRef<Path>>(mut root: PathBuf, parts: &[P]) -> PathBuf {
    for part in parts {
        root.push(part);
    }
    root
}

/// Copies the given content to the system clipboard using the `pbcopy` command.
///
/// # Errors
/// - The clipboard program cannot be spawned.
/// - The clipboard program exits with failure.
pub fn cp_to_system_clipboard(content: &mut &[u8]) -> color_eyre::Result<()> {
    let cmd = "pbcopy";

    let mut pbcopy_child = ytil_cmd::silent_cmd(cmd)
        .stdin(Stdio::piped())
        .spawn()
        .wrap_err_with(|| eyre!("error spawning cmd | cmd={cmd:?}"))?;

    std::io::copy(
        content,
        pbcopy_child
            .stdin
            .as_mut()
            .ok_or_else(|| eyre!("error getting cmd child stdin | cmd={cmd:?}"))?,
    )
    .wrap_err_with(|| eyre!("error copying content to stdin | cmd={cmd:?}"))?;

    if !pbcopy_child
        .wait()
        .wrap_err_with(|| eyre!("error waiting for cmd | cmd={cmd:?}"))?
        .success()
    {
        bail!("error copying to system clipboard | cmd={cmd:?} content={content:#?}");
    }

    Ok(())
}

/// Sets executable permissions (755) on the specified filepath.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - File metadata cannot be read.
/// - Permissions cannot be updated.
pub fn chmod_x<P: AsRef<Path>>(path: P) -> color_eyre::Result<()> {
    let mut perms = std::fs::metadata(&path)
        .wrap_err_with(|| eyre!("error reading metadata | path={}", path.as_ref().display()))?
        .permissions();

    perms.set_mode(0o755);

    std::fs::set_permissions(&path, perms)
        .wrap_err_with(|| eyre!("error setting permissions | path={}", path.as_ref().display()))?;

    Ok(())
}

/// Sets executable permissions on all files in the specified directory.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - A chmod operation fails.
/// - Directory traversal fails.
pub fn chmod_x_files_in_dir<P: AsRef<Path>>(dir: P) -> color_eyre::Result<()> {
    for target_res in
        std::fs::read_dir(&dir).wrap_err_with(|| eyre!("error reading directory | path={}", dir.as_ref().display()))?
    {
        let target = target_res
            .wrap_err_with(|| eyre!("error getting directory entry"))?
            .path();
        if target.is_file() {
            chmod_x(&target).wrap_err_with(|| eyre!("error setting permissions | path={}", target.display()))?;
        }
    }
    Ok(())
}

/// Creates a symbolic link from the target to the link path, removing any existing file at the link location.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - Creating the symlink fails.
/// - The existing link cannot be removed.
pub fn ln_sf<P: AsRef<Path>>(target: &P, link: &P) -> color_eyre::Result<()> {
    if link
        .as_ref()
        .try_exists()
        .wrap_err_with(|| eyre!("error checking if link exists | link={}", link.as_ref().display()))?
    {
        std::fs::remove_file(link.as_ref())
            .wrap_err_with(|| eyre!("error removing existing link | link={}", link.as_ref().display()))?;
    }
    std::os::unix::fs::symlink(target.as_ref(), link.as_ref()).wrap_err_with(|| {
        eyre!(
            "error creating symlink for target={} link={}",
            target.as_ref().display(),
            link.as_ref().display()
        )
    })?;
    Ok(())
}

/// Creates symbolic links for all files in the target directory to the link directory.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - Creating an individual symlink fails.
/// - Traversing `target_dir` fails.
pub fn ln_sf_files_in_dir<P: AsRef<std::path::Path>>(target_dir: P, link_dir: P) -> color_eyre::Result<()> {
    for target in std::fs::read_dir(&target_dir)
        .wrap_err_with(|| eyre!("error reading directory | path={}", target_dir.as_ref().display()))?
    {
        let target = target.wrap_err_with(|| eyre!("error getting target entry"))?.path();
        if target.is_file() {
            let target_name = target
                .file_name()
                .ok_or_else(|| eyre!("error missing filename for target | path={}", target.display()))?;
            let link = link_dir.as_ref().join(target_name);
            ln_sf(&target, &link).wrap_err_with(|| {
                eyre!(
                    "error creating symlink | target={} link={}",
                    target.display(),
                    link.display()
                )
            })?;
        }
    }
    Ok(())
}

/// Atomically copies a file from `from` to `to`.
///
/// The content is first written to a uniquely named temporary sibling (with
/// PID and timestamp) and then moved into place with [`std::fs::rename`]. This
/// minimizes the window where readers could observe a partially written file.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - `from` Does not exist.
/// - The atomic rename fails.
/// - The destination's parent directory or file name cannot be resolved.
/// - The temporary copy fails.
pub fn atomic_cp(from: &Path, to: &Path) -> color_eyre::Result<()> {
    if !from.exists() {
        return Err(eyre!("error missing source file | path={}", from.display()));
    }

    let tmp_name = format!(
        "{}.tmp-{}-{}",
        to.file_name()
            .ok_or_else(|| eyre!("error getting file name | path={}", to.display()))?
            .to_string_lossy(),
        std::process::id(),
        Utc::now().to_rfc3339()
    );
    let tmp_path = to
        .parent()
        .ok_or_else(|| eyre!("error missing parent directory | path={}", to.display()))?
        .join(tmp_name);

    std::fs::copy(from, &tmp_path).with_context(|| {
        format!(
            "error copying file to temp | from={} temp={}",
            from.display(),
            tmp_path.display()
        )
    })?;
    std::fs::rename(&tmp_path, to)
        .with_context(|| format!("error renaming file | from={} to={}", tmp_path.display(), to.display()))?;

    Ok(())
}

/// Resolve workspace root directory.
///
/// Ascends three levels from this crate's manifest.
///
/// # Returns
/// Absolute path to workspace root containing top-level `Cargo.toml`.
///
/// # Errors
/// - Directory traversal fails (unexpected layout).
pub fn get_workspace_root() -> color_eyre::Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or_eyre(format!(
            "cannot get workspace root | manifest_dir={}",
            manifest_dir.display()
        ))?
        .to_path_buf())
}

/// Recursively find files matching a predicate (breadth-first)
///
/// Performs a breadth-first traversal starting at `dir`, skipping directories for which
/// `skip_dir_fn` returns true, and collecting file paths for which `matching_file_fn` returns true.
///
/// # Arguments
/// - `dir` Root directory to start traversal.
/// - `matching_file_fn` Predicate applied to each file entry; include path when it returns true.
/// - `skip_dir_fn` Predicate applied to each directory entry; skip descent when it returns true.
///
/// # Returns
/// Vector of absolute file paths (discovery order unspecified; currently breadth-first).
///
/// # Errors
/// - A directory cannot be read.
/// - File type metadata for an entry cannot be determined.
/// - Any underlying filesystem I/O error occurs during traversal.
///
/// # Performance
/// Uses an in-memory queue (BFS). For extremely deep trees consider a streaming iterator variant;
/// current implementation favors simplicity over incremental output.
///
/// # Future Work
/// - Provide an iterator adapter (`impl Iterator<Item = PathBuf>`), avoiding collecting all results.
/// - Optional parallel traversal behind a feature flag for large repositories.
pub fn find_matching_files_recursively_in_dir(
    dir: &Path,
    matching_file_fn: impl Fn(&DirEntry) -> bool,
    skip_dir_fn: impl Fn(&DirEntry) -> bool,
) -> color_eyre::Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    let mut queue = VecDeque::from([dir.to_path_buf()]);

    while let Some(dir) = queue.pop_front() {
        for entry in
            std::fs::read_dir(&dir).wrap_err_with(|| eyre!("error reading directory | path={}", dir.display()))?
        {
            let entry = entry.wrap_err_with(|| eyre!("error getting entry"))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .wrap_err_with(|| eyre!("error getting file type | entry={}", path.display()))?;

            if file_type.is_file() {
                if matching_file_fn(&entry) {
                    manifests.push(path);
                }
                continue;
            }

            if skip_dir_fn(&entry) {
                continue;
            }
            queue.push_back(path);
        }
    }

    Ok(manifests)
}

/// Opens the given argument using the system's default opener.
///
/// # Arguments
/// - `arg` The argument to open (e.g., URL, or file path).
///
/// # Returns
/// Returns `Ok(())` if the command executes successfully.
///
/// # Errors
/// - The `open` command fails to execute.
/// - The `open` command exits with a non-zero status.
pub fn open(arg: &str) -> color_eyre::Result<()> {
    let cmd = "open";
    Command::new("sh")
        .arg("-c")
        .arg(format!("{cmd} {arg}"))
        .status()
        .wrap_err_with(|| eyre!("error running cmd | cmd={cmd:?} arg={arg:?}"))?
        .exit_ok()
        .wrap_err_with(|| eyre!("error cmd exit not ok | cmd={cmd:?} arg={arg:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_cp_copies_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        std::fs::write(&src, b"hello").unwrap();

        let res = atomic_cp(&src, &dst);

        assert2::let_assert!(Ok(()) = res);
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello");
    }

    #[test]
    fn atomic_cp_errors_when_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("missing.txt");
        let dst = dir.path().join("dst.txt");

        let res = atomic_cp(&src, &dst);

        assert2::let_assert!(Err(err) = res);
        assert!(err.to_string().contains("error missing source file"));
    }

    #[test]
    fn find_matching_files_recursively_in_dir_returns_the_expected_paths() {
        let dir = tempfile::tempdir().unwrap();
        // layout: a/, a/b/, c.txt, a/b/d.txt
        std::fs::create_dir(dir.path().join("a")).unwrap();
        std::fs::create_dir(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("c.txt"), b"c").unwrap();
        std::fs::write(dir.path().join("a/b/d.txt"), b"d").unwrap();

        let res = find_matching_files_recursively_in_dir(
            dir.path(),
            |e| e.path().extension().and_then(|s| s.to_str()) == Some("txt"),
            |_| false,
        );
        assert2::let_assert!(Ok(mut found) = res);
        found.sort();

        let mut expected = vec![dir.path().join("c.txt"), dir.path().join("a/b/d.txt")];
        expected.sort();
        assert_eq!(found, expected);
    }
}
