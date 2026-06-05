use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use muxr_core::SessionPaths;
use rootcause::prelude::ResultExt;
use rootcause::report;

const GROUP_OR_OTHER_PERMISSIONS_MASK: u32 = 0o077;
pub const PRIVATE_DIR_MODE: u32 = 0o700;
pub const PRIVATE_SOCKET_MODE: u32 = 0o600;

pub struct ServerFilesGuard {
    pub paths: SessionPaths,
}

impl Drop for ServerFilesGuard {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.paths.socket));
        drop(fs::remove_file(&self.paths.pid));
    }
}

pub fn prepare_session_dirs(paths: &SessionPaths) -> rootcause::Result<()> {
    let sessions_root = paths
        .root
        .parent()
        .ok_or_else(|| report!("muxr session root has no parent"))?;
    let socket_root = paths
        .socket
        .parent()
        .ok_or_else(|| report!("muxr socket path has no parent"))?;
    let state_root = socket_root
        .parent()
        .ok_or_else(|| report!("muxr socket root has no parent"))?;

    // Socket names are deterministic, so every muxr-owned directory that can expose them must be private.
    for (path, label) in [
        (state_root, "state root"),
        (sessions_root, "sessions root"),
        (socket_root, "socket root"),
        (paths.root.as_path(), "session root"),
        (paths.panes.as_path(), "panes root"),
    ] {
        self::ensure_private_dir(path, label)?;
    }

    Ok(())
}

pub fn secure_socket_file(path: &Path) -> rootcause::Result<()> {
    // The directory is private, but the socket itself should not be group/other accessible if copied or moved.
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_SOCKET_MODE))
        .context("failed to secure muxr socket file permissions")?;
    self::validate_private_mode(path, "socket file", PRIVATE_SOCKET_MODE)
}

fn ensure_private_dir(path: &Path, label: &str) -> rootcause::Result<()> {
    fs::create_dir_all(path).context(format!("failed to create muxr {label}"))?;
    let metadata = fs::symlink_metadata(path).context(format!("failed to inspect muxr {label}"))?;
    if metadata.file_type().is_symlink() {
        return Err(report!("unsafe muxr directory")
            .attach(format!("label={label}"))
            .attach("reason=symlinks are not allowed")
            .attach(format!("path={}", path.display())));
    }
    if !metadata.is_dir() {
        return Err(report!("unsafe muxr directory")
            .attach(format!("label={label}"))
            .attach("reason=path is not a directory")
            .attach(format!("path={}", path.display())));
    }

    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIR_MODE))
        .context(format!("failed to secure muxr {label} permissions"))?;
    self::validate_private_mode(path, label, PRIVATE_DIR_MODE)
}

fn validate_private_mode(path: &Path, label: &str, expected_mode: u32) -> rootcause::Result<()> {
    let mode = fs::metadata(path)
        .context(format!("failed to read muxr {label} permissions"))?
        .permissions()
        .mode()
        & 0o777;

    if mode & GROUP_OR_OTHER_PERMISSIONS_MASK != 0 {
        return Err(report!("unsafe muxr permissions")
            .attach(format!("label={label}"))
            .attach(format!("expected={expected_mode:o}"))
            .attach(format!("actual={mode:o}"))
            .attach(format!("path={}", path.display())));
    }

    Ok(())
}
