//! Provide cohesive system helpers: args, paths, symlinks, permissions, atomic copy, clipboard.
//!
//! Offer small utilities for CLI tools: joining thread handles, building home-relative paths,
//! manipulating filesystem entries (chmod, symlinks, atomic copy) and clipboard integration.

#![feature(exit_status_error)]

use std::collections::VecDeque;
use std::ffi::OsStr;
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
use color_eyre::owo_colors::OwoColorize as _;

/// Retrieves command-line arguments excluding the program name, returning them as a [`Vec`] of [`String`].
pub fn get_args() -> Vec<String> {
    let mut args = std::env::args();
    args.next();
    args.collect::<Vec<String>>()
}

/// Joins a thread handle and returns the result, handling join errors as [`eyre::Error`].
/// Awaits a `JoinHandle` and unwraps the inner `Result`.
///
/// # Errors
/// - The task panicked.
/// - The task returned an error.
pub fn join<T>(join_handle: JoinHandle<color_eyre::Result<T>>) -> Result<T, eyre::Error> {
    join_handle
        .join()
        .map_err(|error| eyre!("join error | error={error:#?}"))?
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
    let mut pbcopy_child = ytil_cmd::silent_cmd("pbcopy").stdin(Stdio::piped()).spawn()?;
    std::io::copy(
        content,
        pbcopy_child
            .stdin
            .as_mut()
            .ok_or_else(|| eyre!("cannot get child stdin | cmd=pbcopy"))?,
    )?;
    if !pbcopy_child.wait()?.success() {
        bail!("copy to system clipboard failed | content={content:#?}");
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
    let mut perms = std::fs::metadata(&path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms)?;
    Ok(())
}

/// Sets executable permissions on all files in the specified directory.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - A chmod operation fails.
/// - Directory traversal fails.
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
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - Creating the symlink fails.
/// - The existing link cannot be removed.
pub fn ln_sf<P: AsRef<Path>>(target: P, link: P) -> color_eyre::Result<()> {
    if link.as_ref().try_exists()? {
        std::fs::remove_file(&link)?;
    }
    std::os::unix::fs::symlink(target, &link)?;
    Ok(())
}

/// Creates symbolic links for all files in the target directory to the link directory.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - Creating an individual symlink fails.
/// - Traversing `target_dir` fails.
pub fn ln_sf_files_in_dir<P: AsRef<std::path::Path>>(target_dir: P, link_dir: P) -> color_eyre::Result<()> {
    for target in std::fs::read_dir(target_dir)? {
        let target = target?.path();
        if target.is_file() {
            let target_name = target
                .file_name()
                .ok_or_else(|| eyre!("missing filename for target | target={target:?}"))?;
            let link = link_dir.as_ref().join(target_name);
            ln_sf(target, link)?;
        }
    }
    Ok(())
}

/// Removes dead symbolic links from the specified directory.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - Directory traversal fails.
/// - Removing a dead symlink fails.
pub fn rm_dead_symlinks(dir: &str) -> color_eyre::Result<()> {
    for entry_res in std::fs::read_dir(dir)? {
        let entry = entry_res?;
        let path = entry.path();

        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() && std::fs::metadata(&path).is_err() {
            std::fs::remove_file(&path)?;
            println!("{} {}", "Deleted dead symlink".cyan().bold(), path.display());
        }
    }
    Ok(())
}

/// Removes the file at the specified path, ignoring if the file does not exist.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - An unexpected I/O failure (other than [`std::io::ErrorKind::NotFound`]) occurs.
pub fn rm_f<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    std::fs::remove_file(path).or_else(|error| {
        if std::io::ErrorKind::NotFound == error.kind() {
            return Ok(());
        }
        Err(error)
    })
}

/// Outcome of file removal operations.
pub struct RmFilesOutcome {
    /// Paths successfully removed or collected in dry run.
    pub removed: Vec<PathBuf>,
    /// Errors encountered, paired with optional affected paths.
    pub errors: Vec<(Option<PathBuf>, std::io::Error)>,
}

/// Iteratively removes all files with the specified name starting from the given root path, with optional directory
/// exclusions.
///
/// This function uses a depth-first iterative traversal with a stack to avoid recursion limits.
/// It checks each file/symlink and removes those matching the name, skipping any directories listed in `excluded_dirs`.
/// Metadata is read once per path to determine file type, reducing syscalls.
///
/// # Arguments
/// - `root_path` The root directory path to start the removal from.
/// - `file_name` The name of the files to remove.
/// - `excluded_dirs` A list of directory names (as strings) to skip during traversal.
/// - `dry_run` If true, collects paths that would be removed without actually removing them.
///
/// # Returns
/// Returns a tuple `(Vec<PathBuf>, Vec<(Option<PathBuf>, std::io::Error)>)` with the list of removed (or would-be
/// removed in `dry_run`) paths and any errors encountered.
/// Each error is paired with the optional path where it occurred (e.g., during metadata read, traversal, or removal).
///
/// # Performance
/// - Reads metadata once per path using `symlink_metadata`, avoiding redundant syscalls.
/// - Iterative approach prevents stack overflow in deep trees.
/// - Exclusions reduce unnecessary traversal, improving performance for large trees with skipped dirs.
/// - IO-bound; memory usage scales with stack depth and error count.
pub fn rm_matching_files<P: AsRef<Path>>(
    root_path: P,
    file_name: &str,
    excluded_dirs: &[&str],
    dry_run: bool,
) -> RmFilesOutcome {
    fn rm_file(
        path: PathBuf,
        dry_run: bool,
        removed: &mut Vec<PathBuf>,
        errors: &mut Vec<(Option<PathBuf>, std::io::Error)>,
    ) {
        if dry_run {
            removed.push(path);
            return;
        }
        if let Err(error) = std::fs::remove_file(&path) {
            errors.push((Some(path), error));
            return;
        }
        removed.push(path);
    }

    fn handle_symlink(
        path: PathBuf,
        dry_run: bool,
        removed: &mut Vec<PathBuf>,
        errors: &mut Vec<(Option<PathBuf>, std::io::Error)>,
    ) {
        match std::fs::read_link(&path) {
            Ok(target) => rm_file(target, dry_run, removed, errors),
            Err(error) => errors.push((Some(path.clone()), error)),
        }
        rm_file(path, dry_run, removed, errors);
    }

    fn handle_dir(
        path: PathBuf,
        stack: &mut VecDeque<PathBuf>,
        excluded_dirs: &[&str],
        errors: &mut Vec<(Option<PathBuf>, std::io::Error)>,
    ) {
        if let Some(dir_name) = path.file_name().and_then(|n| n.to_str())
            && excluded_dirs.contains(&dir_name)
        {
            return;
        }
        match std::fs::read_dir(&path) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => stack.push_back(entry.path()),
                        Err(error) => errors.push((Some(path.clone()), error)),
                    }
                }
            }
            Err(error) => errors.push((Some(path), error)),
        }
    }

    let mut stack = VecDeque::new();
    stack.push_back(root_path.as_ref().to_path_buf());

    let file_name_os = OsStr::new(file_name);
    let mut removed = vec![];
    let mut errors = vec![];

    while let Some(current_path) = stack.pop_back() {
        match std::fs::symlink_metadata(&current_path) {
            Ok(metadata) => {
                let file_type = metadata.file_type();
                if file_type.is_dir() {
                    handle_dir(current_path, &mut stack, excluded_dirs, &mut errors);
                    continue;
                }
                if current_path.file_name() == Some(file_name_os) {
                    if file_type.is_file() {
                        rm_file(current_path.clone(), dry_run, &mut removed, &mut errors);
                    } else if file_type.is_symlink() {
                        handle_symlink(current_path.clone(), dry_run, &mut removed, &mut errors);
                    }
                }
            }
            Err(error) => errors.push((None, error)),
        }
    }

    RmFilesOutcome { removed, errors }
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
        return Err(eyre!("source file missing | path={}", from.display()));
    }

    let tmp_name = format!(
        "{}.tmp-{}-{}",
        to.file_name()
            .ok_or_else(|| eyre!("cannot get file name | path={}", to.display()))?
            .to_string_lossy(),
        std::process::id(),
        Utc::now().to_rfc3339()
    );
    let tmp_path = to
        .parent()
        .ok_or_else(|| eyre!("missing parent directory | path={}", to.display()))?
        .join(tmp_name);

    std::fs::copy(from, &tmp_path).with_context(|| {
        format!(
            "copying file to temp failed | from={} temp={}",
            from.display(),
            tmp_path.display()
        )
    })?;
    std::fs::rename(&tmp_path, to)
        .with_context(|| format!("rename failed | from={} to={}", tmp_path.display(), to.display()))?;

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
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;

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
    Command::new("open").arg(arg).status()?.exit_ok()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rm_f_is_idempotent_for_missing_path() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        // First remove
        assert2::let_assert!(Ok(()) = rm_f(&path));
        // Second remove, no error
        assert2::let_assert!(Ok(()) = rm_f(&path));
    }

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
        assert!(err.to_string().contains("source file missing"));
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

    #[test]
    fn rm_matching_files_dry_run_collects_paths_without_removing() {
        let dir = tempfile::tempdir().unwrap();
        let ds_store = dir.path().join(".DS_Store");
        std::fs::write(&ds_store, b"dummy").unwrap();

        let RmFilesOutcome { removed, errors } = rm_matching_files(dir.path(), ".DS_Store", &[], true);

        assert_eq!(removed, vec![ds_store.clone()]);
        assert!(errors.is_empty());
        assert!(ds_store.exists()); // Should not be removed
    }

    #[test]
    fn rm_matching_files_removes_matching_files() {
        let dir = tempfile::tempdir().unwrap();
        let ds_store = dir.path().join(".DS_Store");
        std::fs::write(&ds_store, b"dummy").unwrap();

        let RmFilesOutcome { removed, errors } = rm_matching_files(dir.path(), ".DS_Store", &[], false);

        assert_eq!(removed, vec![ds_store.clone()]);
        assert!(errors.is_empty());
        assert!(!ds_store.exists()); // Should be removed
    }

    #[test]
    fn rm_matching_files_excludes_specified_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let excluded_dir = dir.path().join("node_modules");
        std::fs::create_dir(&excluded_dir).unwrap();
        let ds_store_in_excluded = excluded_dir.join(".DS_Store");
        std::fs::write(&ds_store_in_excluded, b"dummy").unwrap();

        let regular_dir = dir.path().join("src");
        std::fs::create_dir(&regular_dir).unwrap();
        let ds_store_in_regular = regular_dir.join(".DS_Store");
        std::fs::write(&ds_store_in_regular, b"dummy").unwrap();

        let RmFilesOutcome { removed, errors } = rm_matching_files(dir.path(), ".DS_Store", &["node_modules"], false);

        assert_eq!(removed, vec![ds_store_in_regular.clone()]);
        assert!(errors.is_empty());
        assert!(ds_store_in_excluded.exists()); // Not removed
        assert!(!ds_store_in_regular.exists()); // Removed
    }

    #[test]
    fn rm_matching_files_handles_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        let sub_dir = dir.path().join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        let ds_store = sub_dir.join(".DS_Store");
        std::fs::write(&ds_store, b"dummy").unwrap();

        let RmFilesOutcome { removed, errors } = rm_matching_files(dir.path(), ".DS_Store", &[], false);

        assert_eq!(removed, vec![ds_store.clone()]);
        assert!(errors.is_empty());
        assert!(!ds_store.exists());
    }

    #[test]
    fn rm_matching_files_collects_errors_for_unreadable_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let unreadable_dir = dir.path().join("unreadable");
        std::fs::create_dir(&unreadable_dir).unwrap();

        let RmFilesOutcome { removed, errors } = rm_matching_files("/non/existent/path", ".DS_Store", &[], false);

        assert!(removed.is_empty());
        assert!(!errors.is_empty());
        // Check that error has None path for metadata failure
        assert!(errors.iter().any(|(path, _)| path.is_none()));
    }

    #[test]
    fn rm_matching_files_removes_symlink_and_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"content").unwrap();
        let symlink = dir.path().join(".DS_Store");
        std::os::unix::fs::symlink(&target, &symlink).unwrap();

        let RmFilesOutcome { removed, errors } = rm_matching_files(dir.path(), ".DS_Store", &[], false);

        assert_eq!(removed.len(), 2);
        assert!(errors.is_empty());
        assert!(removed.contains(&symlink));
        assert!(removed.contains(&target));
        assert!(!symlink.exists());
        assert!(!target.exists());
    }
}
