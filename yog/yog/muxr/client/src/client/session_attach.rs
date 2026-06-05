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

use super::session_start;

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
) -> rootcause::Result<AttachedSession> {
    let paths = SessionPaths::from_home(session)?;

    match self::attach(session, &paths, terminal_size.clone()).await {
        Ok(attached_session) => return Ok(attached_session),
        Err(attach_failure) => {
            handle_attach_failure(attach_failure)?;
            session_start::cleanup_stale_session_files(&paths)?;
        }
    }

    session_start::spawn_server_process(session, server_executable)?;
    self::attach_started_server(session, &paths, terminal_size).await
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
) -> rootcause::Result<AttachedSession> {
    let started_at = Instant::now();

    loop {
        match self::attach(session, paths, terminal_size.clone()).await {
            Ok(attached_session) => return Ok(attached_session),
            Err(AttachFailure::Rejected(error)) => return Err(error),
            Err(AttachFailure::Unusable(error)) => {
                // Socket path creation can win the race against listener readiness after spawning the server.
                if started_at.elapsed() > SERVER_READY_TIMEOUT {
                    return Err(error);
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use muxr_core::ServerError;
    use muxr_core::SessionPaths;
    use muxr_transport::ServerListener;

    use super::*;

    #[test]
    fn test_handle_attach_failure_when_server_rejects_and_pid_is_missing_returns_error() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (_, paths) = self::session_paths(tempdir.path(), "work")?;
            fs::create_dir_all(&paths.root)?;
            let _listener = ServerListener::bind(&paths.socket)?;

            assert2::assert!(handle_attach_failure(AttachFailure::Rejected(report!("already attached"))).is_err());
            assert2::assert!(paths.socket.exists());
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
                assert2::assert!(matches!(
                    connection.recv_request().await?,
                    Some(ClientRequest::Attach(_))
                ));
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

            assert2::assert!(attach_error.to_string().contains("muxr server rejected attach"));
            handle
                .await
                .map_err(|error| report!("muxr rejected attach test task panicked").attach(format!("{error}")))??;
            Ok(())
        })
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
