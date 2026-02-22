use std::collections::VecDeque;
use std::fs::DirEntry;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use chrono::Utc;
use rootcause::prelude::ResultExt as _;
use rootcause::report;
use serde::Serialize;
use ytil_cmd::CmdExt as _;

/// Raw filesystem / MIME classification result returned by [`exec_file_cmd`].
#[derive(Clone, Serialize)]
pub enum FileCmdOutput {
    /// Path identified as a binary file.
    BinaryFile(String),
    /// Path identified as a text (plain / CSV) file.
    TextFile(String),
    /// Path identified as a directory.
    Directory(String),
    /// Path that does not exist.
    NotFound(String),
    /// Path whose type could not be determined.
    Unknown(String),
}

/// Execute the system `file -I` command for `path` and classify the MIME output
/// into a [`FileCmdOutput`].
///
/// Used to distinguish:
/// - directories
/// - text files
/// - binary files
/// - missing paths
/// - unknown types
///
/// # Errors
/// - launching or waiting on the `file` command fails
/// - the command exits with non-success
/// - standard output cannot be decoded as valid UTF-8
pub fn exec_file_cmd(path: &str) -> rootcause::Result<FileCmdOutput> {
    let stdout_bytes = Command::new("file").args(["-I", path]).exec()?.stdout;
    let stdout = std::str::from_utf8(&stdout_bytes)?.to_lowercase();
    if stdout.contains(" inode/directory;") {
        return Ok(FileCmdOutput::Directory(path.to_owned()));
    }
    if stdout.contains(" text/") || stdout.contains(" application/json") {
        return Ok(FileCmdOutput::TextFile(path.to_owned()));
    }
    if stdout.contains(" application/") {
        return Ok(FileCmdOutput::BinaryFile(path.to_owned()));
    }
    if stdout.contains(" no such file or directory") {
        return Ok(FileCmdOutput::NotFound(path.to_owned()));
    }
    Ok(FileCmdOutput::Unknown(path.to_owned()))
}

/// Creates a symbolic link from the target to the link path, removing any existing file at the link location.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - Creating the symlink fails.
/// - The existing link cannot be removed.
pub fn ln_sf<P: AsRef<Path>>(target: &P, link: &P) -> rootcause::Result<()> {
    // Remove atomically without check-then-remove TOCTOU race, ignoring NotFound
    match std::fs::remove_file(link.as_ref()) {
        Ok(()) => {}
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            Err(e)
                .context("error removing existing link")
                .attach_with(|| format!("link={}", link.as_ref().display()))?;
        }
    }
    std::os::unix::fs::symlink(target.as_ref(), link.as_ref())
        .context("error creating symlink")
        .attach_with(|| format!("target={} link={}", target.as_ref().display(), link.as_ref().display()))?;
    Ok(())
}

/// Creates symbolic links for all files in the target directory to the link directory.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - Creating an individual symlink fails.
/// - Traversing `target_dir` fails.
pub fn ln_sf_files_in_dir<P: AsRef<std::path::Path>>(target_dir: P, link_dir: P) -> rootcause::Result<()> {
    for target in std::fs::read_dir(&target_dir)
        .context("error reading directory")
        .attach_with(|| format!("path={}", target_dir.as_ref().display()))?
    {
        let target = target.context("error getting target entry")?.path();
        if target.is_file() {
            let target_name = target
                .file_name()
                .ok_or_else(|| report!("error missing filename for target"))
                .attach_with(|| format!("path={}", target.display()))?;
            let link = link_dir.as_ref().join(target_name);
            ln_sf(&target, &link)
                .context("error linking file from directory")
                .attach_with(|| format!("target={} link={}", target.display(), link.display()))?;
        }
    }
    Ok(())
}

/// Copies the given content to the system clipboard using the `pbcopy` command (macOS only).
///
/// # Errors
/// - The clipboard program cannot be spawned.
/// - The clipboard program exits with failure.
pub fn cp_to_system_clipboard(content: &mut &[u8]) -> rootcause::Result<()> {
    let cmd = "pbcopy";

    let mut pbcopy_child = ytil_cmd::silent_cmd(cmd)
        .stdin(Stdio::piped())
        .spawn()
        .context("error spawning cmd")
        .attach_with(|| format!("cmd={cmd:?}"))?;

    std::io::copy(
        content,
        pbcopy_child
            .stdin
            .as_mut()
            .ok_or_else(|| report!("error getting cmd child stdin"))
            .attach_with(|| format!("cmd={cmd:?}"))?,
    )
    .context("error copying content to stdin")
    .attach_with(|| format!("cmd={cmd:?}"))?;

    if !pbcopy_child
        .wait()
        .context("error waiting for cmd")
        .attach_with(|| format!("cmd={cmd:?}"))?
        .success()
    {
        Err(report!("error copying to system clipboard"))
            .attach_with(|| format!("cmd={cmd:?} content={content:#?}"))?;
    }

    Ok(())
}

/// Sets executable permissions (755) on the specified filepath.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - File metadata cannot be read.
/// - Permissions cannot be updated.
pub fn chmod_x<P: AsRef<Path>>(path: P) -> rootcause::Result<()> {
    let mut perms = std::fs::metadata(&path)
        .context("error reading metadata")
        .attach_with(|| format!("path={}", path.as_ref().display()))?
        .permissions();

    perms.set_mode(0o755);

    std::fs::set_permissions(&path, perms)
        .context("error setting permissions")
        .attach_with(|| format!("path={}", path.as_ref().display()))?;

    Ok(())
}

/// Sets executable permissions on all files in the specified directory.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
/// - A chmod operation fails.
/// - Directory traversal fails.
pub fn chmod_x_files_in_dir<P: AsRef<Path>>(dir: P) -> rootcause::Result<()> {
    for target_res in std::fs::read_dir(&dir)
        .context("error reading directory")
        .attach_with(|| format!("path={}", dir.as_ref().display()))?
    {
        let target = target_res.context("error getting directory entry")?.path();
        if target.is_file() {
            chmod_x(&target)
                .context("error setting file permissions in directory")
                .attach_with(|| format!("path={}", target.display()))?;
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
/// - `from` does not exist (error from `std::fs::copy`).
/// - The atomic rename fails.
/// - The destination's parent directory or file name cannot be resolved.
/// - The temporary copy fails.
pub fn atomic_cp(from: &Path, to: &Path) -> rootcause::Result<()> {
    // Removed explicit existence check to avoid TOCTOU race - let std::fs::copy
    // report the error if the source doesn't exist
    let tmp_name = format!(
        "{}.tmp-{}-{}",
        to.file_name()
            .ok_or_else(|| report!("error getting file name"))
            .attach_with(|| format!("path={}", to.display()))?
            .to_string_lossy(),
        std::process::id(),
        Utc::now().to_rfc3339()
    );
    let tmp_path = to
        .parent()
        .ok_or_else(|| report!("error missing parent directory"))
        .attach_with(|| format!("path={}", to.display()))?
        .join(tmp_name);

    std::fs::copy(from, &tmp_path)
        .context("error copying file to temp")
        .attach_with(|| format!("from={} temp={}", from.display(), tmp_path.display()))?;
    std::fs::rename(&tmp_path, to)
        .context("error renaming file")
        .attach_with(|| format!("from={} to={}", tmp_path.display(), to.display()))?;

    Ok(())
}

/// Recursively find files matching a predicate (breadth-first)
///
/// Performs a breadth-first traversal starting at `dir`, skipping directories for which
/// `skip_dir_fn` returns true, and collecting file paths for which `matching_file_fn` returns true.
///
/// # Errors
/// - Filesystem I/O error during traversal.
pub fn find_matching_recursively_in_dir(
    dir: &Path,
    matching_file_fn: impl Fn(&DirEntry) -> bool,
    skip_dir_fn: impl Fn(&DirEntry) -> bool,
) -> rootcause::Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    let mut queue = VecDeque::from([dir.to_path_buf()]);

    while let Some(dir) = queue.pop_front() {
        for entry in std::fs::read_dir(&dir)
            .context("error reading directory")
            .attach_with(|| format!("path={}", dir.display()))?
        {
            let entry = entry.context("error getting entry")?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .context("error getting file type")
                .attach_with(|| format!("entry={}", path.display()))?;

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

        assert2::assert!(let Ok(()) = res);
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello");
    }

    #[test]
    fn atomic_cp_errors_when_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("missing.txt");
        let dst = dir.path().join("dst.txt");

        let res = atomic_cp(&src, &dst);

        assert2::assert!(let Err(err) = res);
        // Error now comes from std::fs::copy wrapped with context
        assert!(err.to_string().contains("error copying file to temp"));
    }

    #[test]
    fn find_matching_recursively_in_dir_returns_the_expected_paths() {
        let dir = tempfile::tempdir().unwrap();
        // layout: a/, a/b/, c.txt, a/b/d.txt
        std::fs::create_dir(dir.path().join("a")).unwrap();
        std::fs::create_dir(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("c.txt"), b"c").unwrap();
        std::fs::write(dir.path().join("a/b/d.txt"), b"d").unwrap();

        let res = find_matching_recursively_in_dir(
            dir.path(),
            |e| e.path().extension().and_then(|s| s.to_str()) == Some("txt"),
            |_| false,
        );
        assert2::assert!(let Ok(mut found) = res);
        found.sort();

        let mut expected = vec![dir.path().join("c.txt"), dir.path().join("a/b/d.txt")];
        expected.sort();
        assert_eq!(found, expected);
    }
}
