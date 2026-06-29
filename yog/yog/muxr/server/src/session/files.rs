use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use muxr_core::SessionPaths;
use rootcause::prelude::ResultExt;
use rootcause::report;

pub const PRIVATE_DIR_MODE: u32 = 0o700;
pub const PRIVATE_SOCKET_MODE: u32 = 0o600;

pub struct ServerFilesGuard {
    pub paths: SessionPaths,
}

impl Drop for ServerFilesGuard {
    fn drop(&mut self) {
        self::remove_server_file("remove_socket", &self.paths.socket);
        self::remove_server_file("remove_pid", &self.paths.pid);
    }
}

fn remove_server_file(event: &str, path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        // Cleanup can race with explicit delete or partial startup rollback; only other errors leave stale state.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => crate::session::tracing::server::file_cleanup_failed(event, path, &error),
    }
}

pub fn prepare_session_dirs(paths: &SessionPaths) -> rootcause::Result<()> {
    let sessions_root = paths
        .root
        .parent()
        .ok_or_else(|| report!("muxr session root has no parent"))?;
    let logs_root = paths.logs_root()?;
    let socket_root = self::socket_root(paths)?;
    let state_root = self::state_root(paths)?;

    fs::create_dir_all(state_root).context("failed to create muxr state root")?;
    fs::set_permissions(state_root, fs::Permissions::from_mode(PRIVATE_DIR_MODE))
        .context("failed to secure muxr state root permissions")?;

    // muxr is a personal tool: the only permission invariant is the state root mode. Child paths are ordinary state,
    // and user-created symlinks are treated as explicit state relocation.
    for (path, label) in [
        (sessions_root, "sessions root"),
        (logs_root.as_path(), "logs root"),
        (socket_root, "socket root"),
        (paths.root.as_path(), "session root"),
        (paths.panes.as_path(), "panes root"),
    ] {
        fs::create_dir_all(path).context(format!("failed to create muxr {label}"))?;
    }

    Ok(())
}

pub fn secure_socket_file(path: &Path) -> rootcause::Result<()> {
    // The directory is private, but the socket itself should not be group/other accessible if copied or moved.
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_SOCKET_MODE))
        .context("failed to secure muxr socket file permissions")?;
    Ok(())
}

fn state_root(paths: &SessionPaths) -> rootcause::Result<&Path> {
    self::socket_root(paths)?
        .parent()
        .ok_or_else(|| report!("muxr socket root has no parent"))
}

fn socket_root(paths: &SessionPaths) -> rootcause::Result<&Path> {
    paths
        .socket
        .parent()
        .ok_or_else(|| report!("muxr socket path has no parent"))
}

#[cfg(test)]
mod tests {
    use muxr_core::SessionName;
    use muxr_core::SessionPaths;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_prepare_session_dirs_when_state_root_is_public_secures_state_root() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let session = SessionName::default();
        let paths = SessionPaths::from_sessions_root_path(&tempdir.path().join("sessions"), &session)?;
        let state_root = self::state_root(&paths)?;
        fs::create_dir_all(state_root).context("failed to create test state root")?;
        fs::set_permissions(state_root, fs::Permissions::from_mode(0o755))
            .context("failed to make test state root public")?;

        prepare_session_dirs(&paths)?;

        assert_that!(self::mode(state_root)?, eq(PRIVATE_DIR_MODE));
        assert_that!(paths.root.is_dir(), eq(true));
        assert_that!(paths.panes.is_dir(), eq(true));
        assert_that!(paths.logs_root()?.is_dir(), eq(true));
        Ok(())
    }

    #[test]
    fn test_remove_server_file_when_path_is_missing_is_silent() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let session = SessionName::default();

        let log = crate::session::tracing::collect_test_log(&session, || {
            self::remove_server_file("remove_socket", &tempdir.path().join("missing.sock"));
            Ok(())
        })?;

        assert_that!(log, not(contains_substring("kind=\"server_file_cleanup_failed\"")));
        Ok(())
    }

    fn mode(path: &Path) -> rootcause::Result<u32> {
        Ok(fs::metadata(path)
            .context("failed to read test path mode")?
            .permissions()
            .mode()
            & 0o777)
    }
}
