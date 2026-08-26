use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use muxr_core::AttachRequest;
use muxr_core::ClientRequest;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::ServerEvent;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TerminalSize;
use muxr_transport::ClientConnection;
use muxr_transport::ClientEventReader;
use muxr_transport::ClientRequestWriter;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::session::start;

const ATTACH_TIMEOUT: Duration = Duration::from_secs(2);
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(2);

pub struct AttachedSession {
    pub layout: LayoutSnapshot,
    pub pane_regions: PaneRegionsSnapshot,
    pub reader: ClientEventReader,
    pub writer: ClientRequestWriter,
}

enum AttachFailure {
    Rejected(rootcause::Report),
    Unusable(rootcause::Report),
}

pub async fn open_session(
    session: &SessionName,
    terminal_size: TerminalSize,
    server_executable: &Path,
    external_layout: Option<&Path>,
) -> rootcause::Result<AttachedSession> {
    let paths = SessionPaths::from_home(session)?;
    self::open_session_with_paths(session, &paths, terminal_size, server_executable, external_layout).await
}

async fn open_session_with_paths(
    session: &SessionName,
    paths: &SessionPaths,
    terminal_size: TerminalSize,
    server_executable: &Path,
    external_layout: Option<&Path>,
) -> rootcause::Result<AttachedSession> {
    if let Some(external_layout) = external_layout {
        self::guard_external_start_seed(paths, session, external_layout).await?;
    }

    match self::attach(session, paths, terminal_size.clone()).await {
        Ok(attached_session) => return Ok(attached_session),
        Err(attach_failure) => {
            handle_attach_failure(attach_failure)?;
            start::cleanup_stale_session_files(paths)?;
        }
    }

    let spawned_server = start::spawn_server_process(session, paths, server_executable, external_layout)?;
    self::attach_started_server(session, paths, terminal_size, &spawned_server.log_locator).await
}

async fn attach(
    session: &SessionName,
    paths: &SessionPaths,
    terminal_size: TerminalSize,
) -> Result<AttachedSession, AttachFailure> {
    let mut connection = self::connect_with_timeout(paths).await?;

    tokio::time::timeout(
        ATTACH_TIMEOUT,
        connection.send_request(&ClientRequest::Attach(AttachRequest {
            session: session.clone(),
            terminal_size,
        })),
    )
    .await
    .map_err(|_| AttachFailure::Unusable(report!("timed out writing muxr attach request")))?
    .map_err(AttachFailure::Unusable)?;

    let (layout, pane_regions) = match tokio::time::timeout(ATTACH_TIMEOUT, connection.recv_event())
        .await
        .map_err(|_| AttachFailure::Unusable(report!("timed out waiting for muxr attach response")))?
        .map_err(AttachFailure::Unusable)?
    {
        Some(ServerEvent::Attached(attached)) => (attached.layout, attached.pane_regions),
        Some(ServerEvent::Error(error)) => {
            return Err(AttachFailure::Rejected(
                report!("muxr server rejected attach")
                    .attach(format!("code={}", error.code()))
                    .attach(format!("msg={}", error.msg())),
            ));
        }
        Some(event) => {
            return Err(AttachFailure::Unusable(
                report!("unexpected muxr server attach event").attach(format!("{event:?}")),
            ));
        }
        None => return Err(AttachFailure::Unusable(report!("muxr server closed before attach"))),
    };

    let (reader, writer) = connection.split();
    Ok(AttachedSession {
        layout,
        pane_regions,
        reader,
        writer,
    })
}

async fn connect_with_timeout(paths: &SessionPaths) -> Result<ClientConnection, AttachFailure> {
    tokio::time::timeout(ATTACH_TIMEOUT, ClientConnection::connect(&paths.socket))
        .await
        .map_err(|_| AttachFailure::Unusable(report!("timed out connecting muxr session socket")))?
        .map_err(AttachFailure::Unusable)
}

async fn attach_started_server(
    session: &SessionName,
    paths: &SessionPaths,
    terminal_size: TerminalSize,
    server_log_locator: &start::ServerLogLocator,
) -> rootcause::Result<AttachedSession> {
    let started_at = Instant::now();

    loop {
        match self::attach(session, paths, terminal_size.clone()).await {
            Ok(attached_session) => return Ok(attached_session),
            Err(AttachFailure::Rejected(error)) => return Err(error),
            Err(AttachFailure::Unusable(error)) => {
                // Socket path creation can win the race against listener readiness after spawning the server.
                if started_at.elapsed() > SERVER_READY_TIMEOUT {
                    return Err(self::server_startup_failure_error(error, server_log_locator));
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn server_startup_failure_error(
    error: rootcause::Report,
    server_log_locator: &start::ServerLogLocator,
) -> rootcause::Report {
    // The server owns log timestamp generation, and startup failures may happen before attach can return it. Use the
    // spawned pid as the stable client-known part of the debug hint instead of scanning logs during startup failure.
    error
        .attach("muxr server did not become attachable after start")
        .attach(format!("server_pid={}", server_log_locator.pid))
        .attach(format!("logs_dir={}", server_log_locator.logs_dir.display()))
        .attach(format!("log_pattern={}", server_log_locator.file_pattern))
}

fn handle_attach_failure(attach_failure: AttachFailure) -> rootcause::Result<()> {
    match attach_failure {
        AttachFailure::Rejected(attach_error) => {
            // A structured muxr rejection proves the socket is live even if pid metadata is missing or stale.
            Err(attach_error).attach("socket returned a structured muxr response")
        }
        AttachFailure::Unusable(attach_error) => {
            // Even stale/incompatible servers may still answer Ping; an unusable attach is the compatibility signal.
            drop(attach_error);
            Ok(())
        }
    }
}

async fn guard_external_start_seed(
    paths: &SessionPaths,
    session: &SessionName,
    external_layout: &Path,
) -> rootcause::Result<()> {
    match fs::read(&paths.layout) {
        Ok(_) => {
            return Err(self::external_layout_existing_session_error(
                session,
                external_layout,
                "persisted-layout",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to read muxr session layout metadata")?,
    }

    if crate::session::list::session_state_async(paths).await? == crate::session::list::SessionState::Live {
        return Err(self::external_layout_existing_session_error(
            session,
            external_layout,
            "live",
        ));
    }

    Ok(())
}

fn external_layout_existing_session_error(
    session: &SessionName,
    external_layout: &Path,
    state: &str,
) -> rootcause::Report {
    report!("muxr external layout can only seed a new session")
        .attach(format!("session={session}"))
        .attach(format!("layout={}", external_layout.display()))
        .attach(format!("state={state}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use muxr_core::AttachAccepted;
    use muxr_core::LayoutSnapshot;
    use muxr_core::PaneId;
    use muxr_core::PaneMouseMode;
    use muxr_core::PaneRegionSnapshot;
    use muxr_core::PaneRegionsSnapshot;
    use muxr_core::PaneSnapshot;
    use muxr_core::ServerError;
    use muxr_core::SessionPaths;
    use muxr_core::TabId;
    use muxr_core::TabSnapshot;
    use muxr_core::TrackedProcessState;
    use muxr_transport::ServerListener;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_guard_external_start_seed_when_layout_metadata_exists_returns_error() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let layout = Path::new("../.config/muxr/layouts/work.json");
        fs::create_dir_all(&paths.root)?;
        fs::write(&paths.layout, b"not necessarily valid json")?;

        let error = self::runtime()?
            .block_on(guard_external_start_seed(&paths, &session, layout))
            .expect_err("expected persisted layout to block external layout seed");

        assert_that!(
            error.to_string(),
            contains_substring("muxr external layout can only seed a new session")
        );
        assert_that!(error.to_string(), contains_substring("state=persisted-layout"));
        Ok(())
    }

    #[test]
    fn test_guard_external_start_seed_when_socket_is_live_returns_error() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let layout = Path::new("../.config/muxr/layouts/work.json");
        fs::create_dir_all(&paths.root)?;
        let runtime = self::runtime()?;
        let error = runtime.block_on(async {
            let listener = ServerListener::bind(&paths.socket)?;
            let handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                assert_that!(connection.recv_request().await?, eq(Some(ClientRequest::Ping)));
                connection.send_event(&ServerEvent::Pong).await?;
                Ok::<(), rootcause::Report>(())
            });

            let error = guard_external_start_seed(&paths, &session, layout)
                .await
                .expect_err("expected live session to block external layout seed");
            handle
                .await
                .map_err(|error| report!("muxr live layout guard test task panicked").attach(format!("{error}")))??;
            Ok::<_, rootcause::Report>(error)
        })?;

        assert_that!(
            error.to_string(),
            contains_substring("muxr external layout can only seed a new session")
        );
        assert_that!(error.to_string(), contains_substring("state=live"));
        Ok(())
    }

    #[test]
    fn test_handle_attach_failure_when_server_rejects_and_pid_is_missing_returns_error() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let _listener = ServerListener::bind(&paths.socket)?;

            assert_that!(
                handle_attach_failure(AttachFailure::Rejected(report!("already attached"))),
                err(anything())
            );
            assert_that!(paths.socket.exists(), eq(true));
            Ok(())
        })
    }

    #[test]
    fn test_attach_when_server_rejects_returns_rejected_error() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                assert_that!(
                    connection.recv_request().await?,
                    some(matches_pattern!(ClientRequest::Attach(anything())))
                );
                connection
                    .send_event(&ServerEvent::Error(ServerError::ClientAlreadyAttached))
                    .await?;
                Ok::<(), rootcause::Report>(())
            });

            let attach_error = attach(&session, &paths, TerminalSize::new(80, 24)?).await.map_or_else(
                |failure| match failure {
                    AttachFailure::Rejected(error) | AttachFailure::Unusable(error) => error,
                },
                |_| report!("expected rejected attach"),
            );

            assert_that!(
                attach_error.to_string(),
                contains_substring("muxr server rejected attach")
            );
            handle
                .await
                .map_err(|error| report!("muxr rejected attach test task panicked").attach(format!("{error}")))??;
            Ok(())
        })
    }

    #[test]
    fn test_open_session_when_live_session_exists_with_missing_runner_attaches_without_spawning()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let listener = ServerListener::bind(&paths.socket)?;
            let handle = tokio::spawn(async move {
                let mut connection = listener.accept().await?;
                assert_that!(
                    connection.recv_request().await?,
                    some(matches_pattern!(ClientRequest::Attach(anything())))
                );
                connection.send_event(&self::attached_event()?).await?;
                Ok::<(), rootcause::Report>(())
            });
            let missing_runner = tempdir.path().join("missing-muxr-server");

            let attached_session =
                open_session_with_paths(&session, &paths, TerminalSize::new(80, 24)?, &missing_runner, None).await?;

            assert_that!(attached_session.layout.active_tab(), eq(&TabId::new(1)?));
            handle
                .await
                .map_err(|error| report!("muxr live attach test task panicked").attach(format!("{error}")))??;
            Ok(())
        })
    }

    #[test]
    fn test_open_session_when_started_server_exits_before_attach_returns_log_locator() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let runner = tempdir.path().join("muxr-server");
            fs::write(&runner, "#!/bin/sh\nexit 17\n").context("failed to write fake muxr server")?;
            fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
                .context("failed to make fake muxr server executable")?;

            let open_result =
                open_session_with_paths(&session, &paths, TerminalSize::new(80, 24)?, &runner, None).await;
            assert_that!(
                open_result.as_ref().map(|_| ()).map_err(ToString::to_string),
                err(contains_substring("muxr server did not become attachable after start"))
            );
            let error = open_result.map_or_else(|error| error.to_string(), |_| String::new());
            assert_that!(error, contains_substring("server_pid="));
            assert_that!(
                error,
                contains_substring(format!("logs_dir={}", paths.logs_root()?.display()))
            );
            assert_that!(error, contains_substring("log_pattern=work-*-"));
            assert_that!(error, not(contains_substring("server_log=")));
            assert_that!(error, contains_substring(".log"));
            Ok(())
        })
    }

    fn attached_event() -> rootcause::Result<ServerEvent> {
        let pane_id = PaneId::new(1)?;
        let tab_id = TabId::new(1)?;
        let pane = PaneSnapshot {
            cmd_label: None,
            cwd: "/tmp".to_string(),
            focus_seq: 0,
            id: pane_id,
            title: "shell".to_string(),
            tracked_process_state: TrackedProcessState::None,
        };
        let tab = TabSnapshot::new(tab_id, "default", pane_id, vec![pane])?;
        let layout = LayoutSnapshot::new(tab_id, vec![tab])?;
        let region = PaneRegionSnapshot::new(pane_id, 0, 0, 80, 24, PaneMouseMode::None, 0)?;
        let pane_regions = PaneRegionsSnapshot::new(vec![region])?;
        Ok(ServerEvent::Attached(AttachAccepted { layout, pane_regions }))
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

    fn runtime() -> rootcause::Result<tokio::runtime::Runtime> {
        Ok(tokio::runtime::Runtime::new().context("failed to build muxr test runtime")?)
    }
}
