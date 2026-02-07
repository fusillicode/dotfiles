use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;

use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt as _;

/// Outcome of file removal operations.
pub struct RmFilesOutcome {
    /// Paths successfully removed or collected in dry run.
    pub removed: Vec<PathBuf>,
    /// Errors encountered, paired with optional affected paths.
    pub errors: Vec<(Option<PathBuf>, std::io::Error)>,
}

/// Removes dead symbolic links from the specified directory.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - Directory traversal fails.
/// - Removing a dead symlink fails.
pub fn rm_dead_symlinks(dir: &str) -> rootcause::Result<()> {
    for entry_res in std::fs::read_dir(dir)
        .context("error reading directory")
        .attach_with(|| format!("path={dir:?}"))?
    {
        let entry = entry_res.context("error getting entry")?;
        let path = entry.path();

        let metadata = std::fs::symlink_metadata(&path)
            .context("error reading symlink metadata")
            .attach_with(|| format!("path={}", path.display()))?;
        if metadata.file_type().is_symlink() && std::fs::metadata(&path).is_err() {
            std::fs::remove_file(&path)
                .context("error removing dead symlink")
                .attach_with(|| format!("path={}", path.display()))?;
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
    std::fs::remove_file(path).or_else(|err| {
        if std::io::ErrorKind::NotFound == err.kind() {
            return Ok(());
        }
        Err(err)
    })
}

/// Iteratively removes all files with the specified name starting from the given root path, with optional directory
/// exclusions.
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
        if let Err(err) = std::fs::remove_file(&path) {
            errors.push((Some(path), err));
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
            Err(err) => errors.push((Some(path.clone()), err)),
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
                        Err(err) => errors.push((Some(path.clone()), err)),
                    }
                }
            }
            Err(err) => errors.push((Some(path), err)),
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
            Err(err) => errors.push((None, err)),
        }
    }

    RmFilesOutcome { removed, errors }
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
