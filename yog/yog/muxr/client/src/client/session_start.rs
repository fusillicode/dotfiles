use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use muxr_core::INTERNAL_SERVER_ARG;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use rootcause::prelude::ResultExt;

pub fn cleanup_stale_session_files(paths: &SessionPaths) -> rootcause::Result<()> {
    // Structured rejections stop before cleanup; unusable attach failures may be stale incompatible servers.
    self::remove_file_if_exists(&paths.socket)?;
    self::remove_file_if_exists(&paths.pid)?;
    Ok(())
}

pub fn spawn_server_process(session: &SessionName, server_executable: &Path) -> rootcause::Result<()> {
    let mut cmd = self::server_cmd(session, server_executable);

    let child = cmd.spawn().context("failed to spawn muxr internal server")?;
    drop(child);
    Ok(())
}

fn server_cmd(session: &SessionName, server_executable: &Path) -> Command {
    let mut cmd = Command::new(server_executable);
    cmd.arg(INTERNAL_SERVER_ARG)
        .arg(session.as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);

    cmd
}

fn remove_file_if_exists(path: &Path) -> rootcause::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("failed to remove stale muxr file")?,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::net::UnixListener;
    use std::path::Path;

    use muxr_core::INTERNAL_SERVER_ARG;
    use muxr_core::SessionPaths;

    use super::*;

    #[test]
    fn test_cleanup_stale_session_files_when_running_pid_has_missing_socket_removes_pid() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        fs::write(&paths.pid, std::process::id().to_string())?;

        cleanup_stale_session_files(&paths)?;

        assert2::assert!(!paths.pid.exists());
        assert2::assert!(!paths.socket.exists());
        Ok(())
    }

    #[test]
    fn test_cleanup_stale_session_files_when_socket_path_exists_removes_socket_path() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        let _listener = UnixListener::bind(&paths.socket)?;

        cleanup_stale_session_files(&paths)?;

        assert2::assert!(!paths.socket.exists());
        Ok(())
    }

    #[test]
    fn test_server_cmd_uses_supplied_executable_for_internal_server() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let cmd = server_cmd(&session, Path::new("/tmp/custom-muxr"));
        let args: Vec<_> = cmd.get_args().collect();

        pretty_assertions::assert_eq!(cmd.get_program(), OsStr::new("/tmp/custom-muxr"));
        pretty_assertions::assert_eq!(args.as_slice(), [OsStr::new(INTERNAL_SERVER_ARG), OsStr::new("work")]);
        Ok(())
    }

    fn session_paths(base: &Path, raw: &str) -> rootcause::Result<(SessionName, SessionPaths)> {
        let session = raw.parse()?;
        let root = base.join("sessions").join(raw);

        Ok((
            session,
            SessionPaths {
                socket: root.join("server.sock"),
                pid: root.join("server.pid"),
                layout: root.join("layout.json"),
                panes: root.join("panes"),
                root,
            },
        ))
    }
}
