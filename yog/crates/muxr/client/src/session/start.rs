use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use muxr_core::ServerRunnerArgs;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use rootcause::prelude::ResultExt;
use rootcause::report;

pub struct SpawnedServer {
    pub log_locator: ServerLogLocator,
}

pub struct ServerLogLocator {
    pub file_pattern: String,
    pub logs_dir: PathBuf,
    pub pid: u32,
}

pub fn cleanup_stale_session_files(paths: &SessionPaths) -> rootcause::Result<()> {
    // Structured rejections stop before cleanup; unusable attach failures may be stale incompatible servers.
    self::remove_file_if_exists(&paths.socket)?;
    self::remove_file_if_exists(&paths.pid)?;
    Ok(())
}

pub fn spawn_server_process(
    session: &SessionName,
    paths: &SessionPaths,
    server_executable: &Path,
    external_layout: Option<&Path>,
) -> rootcause::Result<SpawnedServer> {
    // Existing sessions attach before this path; validate the runner only when spawning a missing/new server.
    if !server_executable.is_file() {
        return Err(report!("missing muxr server runner")
            .attach(format!("expected={}", server_executable.display()))
            .attach("run the muxr install/build step so muxr-server is installed next to muxr"));
    }
    let logs_dir = paths.logs_root()?;
    let mut cmd = self::server_cmd(session, server_executable, external_layout);

    let child = cmd.spawn().context("failed to spawn muxr server")?;
    let pid = child.id();
    drop(child);
    Ok(SpawnedServer {
        log_locator: ServerLogLocator {
            file_pattern: SessionPaths::server_log_file_pattern(session, pid),
            logs_dir,
            pid,
        },
    })
}

fn server_cmd(session: &SessionName, server_executable: &Path, external_layout: Option<&Path>) -> Command {
    let mut cmd = Command::new(server_executable);
    let runner_args = ServerRunnerArgs {
        external_layout: external_layout.map(Path::to_path_buf),
        session: session.clone(),
    };
    cmd.args(runner_args.argv())
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

    use muxr_core::EXTERNAL_LAYOUT_ARG;
    use muxr_core::SessionPaths;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_cleanup_stale_session_files_when_running_pid_has_missing_socket_removes_pid() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        fs::write(&paths.pid, std::process::id().to_string())?;

        cleanup_stale_session_files(&paths)?;

        assert_that!(paths.pid.exists(), eq(false));
        assert_that!(paths.socket.exists(), eq(false));
        Ok(())
    }

    #[test]
    fn test_cleanup_stale_session_files_when_socket_path_exists_removes_socket_path() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        let _listener = UnixListener::bind(&paths.socket)?;

        cleanup_stale_session_files(&paths)?;

        assert_that!(paths.socket.exists(), eq(false));
        Ok(())
    }

    #[test]
    fn test_server_cmd_uses_supplied_executable_for_server() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let cmd = server_cmd(&session, Path::new("/tmp/custom-muxr"), None);
        let args: Vec<_> = cmd.get_args().collect();

        assert_that!(cmd.get_program(), eq(OsStr::new("/tmp/custom-muxr")));
        assert_that!(args.as_slice(), eq([OsStr::new("work")]));
        Ok(())
    }

    #[test]
    fn test_server_cmd_when_external_layout_is_supplied_passes_layout_to_server() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let layout = Path::new("../.config/muxr/layouts/work.json");
        let cmd = server_cmd(&session, Path::new("/tmp/custom-muxr"), Some(layout));
        let args: Vec<_> = cmd.get_args().collect();

        assert_that!(
            args.as_slice(),
            eq([
                OsStr::new("work"),
                OsStr::new(EXTERNAL_LAYOUT_ARG),
                OsStr::new("../.config/muxr/layouts/work.json")
            ])
        );
        Ok(())
    }

    #[test]
    fn test_spawn_server_process_when_runner_is_missing_returns_error() -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let tempdir = tempfile::tempdir()?;
        let missing_runner = tempdir.path().join("muxr-server");
        let (_, paths) = self::session_paths(tempdir.path(), "work")?;

        assert_that!(
            spawn_server_process(&session, &paths, &missing_runner, None).map(|_| ()),
            err(displays_as(contains_substring("missing muxr server runner")))
        );
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
