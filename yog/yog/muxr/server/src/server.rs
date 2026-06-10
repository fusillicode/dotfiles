use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use muxr_config::MuxrConfig;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TerminalSize;
use muxr_transport::ServerConnection;
use muxr_transport::ServerListener;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::pty::ShellCmd;
use crate::session_files::ServerFilesGuard;
use crate::session_files::prepare_session_dirs;
use crate::session_files::secure_socket_file;
use crate::session_runtime::CLIENT_HANDSHAKE_CHANNEL_LIMIT;
use crate::session_runtime::ReapResult;
use crate::session_runtime::SessionAttachedClientTaskMessage;
use crate::session_runtime::SessionClientHandshake;
use crate::session_runtime::SessionHandshakeOutcome;
use crate::session_runtime::SessionRuntime;
use crate::session_runtime::SessionRuntimeShutdownMessage;
use crate::sessions_delete::DeleteSessions;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

struct SessionHandshakeContext<'a> {
    attached_client_task_sender: &'a tokio::sync::mpsc::Sender<SessionAttachedClientTaskMessage>,
    config: &'a ServerConfig,
    delete_sessions: &'a Arc<DeleteSessions>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub client_heartbeat_interval: Duration,
    pub client_heartbeat_timeout: Duration,
    pub client_write_timeout: Duration,
    pub external_layout: Option<PathBuf>,
    pub user_config: Arc<MuxrConfig>,
    pub session: SessionName,
    pub paths: SessionPaths,
    max_accepted_connections: Option<usize>,
    pub shell_cmd: ShellCmd,
}

/// Run the muxr server for one internally launched session.
///
/// `external_layout` is a one-shot seed for brand-new sessions; persisted layout metadata remains authoritative.
///
/// # Errors
/// - Server startup, socket IO, PTY setup, or pid file persistence fails.
pub fn serve_session(session: &SessionName, external_layout: Option<PathBuf>) -> rootcause::Result<()> {
    let paths = SessionPaths::from_home(session)?;
    let config = ServerConfig {
        client_heartbeat_interval: CLIENT_HEARTBEAT_INTERVAL,
        client_heartbeat_timeout: CLIENT_HEARTBEAT_TIMEOUT,
        client_write_timeout: CLIENT_WRITE_TIMEOUT,
        external_layout,
        user_config: Arc::new(MuxrConfig::default()),
        session: session.clone(),
        paths,
        max_accepted_connections: None,
        shell_cmd: ShellCmd::default_from_env()?,
    };

    tokio::runtime::Runtime::new()
        .context("failed to build muxr tokio runtime")?
        .block_on(self::serve_async(&config))
}

pub fn unix_timestamp_millis() -> rootcause::Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("failed to read system time for muxr layout metadata")?
        .as_millis();

    Ok(u64::try_from(millis).context("muxr layout metadata timestamp overflowed")?)
}

/// Build metadata for panes spawned from the currently active pane.
///
/// New panes inherit the active pane cwd because the server process cwd does not follow interactive `cd` cmds.
pub fn active_pane_session_metadata(
    config: &ServerConfig,
    layout: &SessionLayout,
) -> rootcause::Result<SessionMetadata> {
    let active_pane_id = layout.active_pane_id()?;
    let cwd = layout
        .pane(active_pane_id)
        .map(|pane| pane.cwd.clone())
        .ok_or_else(|| {
            report!("muxr active pane is missing from server layout").attach(format!("pane_id={active_pane_id}"))
        })?;

    Ok(SessionMetadata {
        cmd_label: config.shell_cmd.label(),
        cwd,
        started_at: self::unix_timestamp_millis()?,
    })
}

impl TryFrom<&ServerConfig> for SessionMetadata {
    type Error = rootcause::Report;

    fn try_from(config: &ServerConfig) -> rootcause::Result<Self> {
        Ok(Self {
            cmd_label: config.shell_cmd.label(),
            cwd: std::env::current_dir()
                .context("failed to read muxr server cwd")?
                .to_string_lossy()
                .into_owned(),
            started_at: self::unix_timestamp_millis()?,
        })
    }
}

async fn serve_async(config: &ServerConfig) -> rootcause::Result<()> {
    if matches!(config.max_accepted_connections, Some(0)) {
        return Ok(());
    }

    prepare_session_dirs(&config.paths)?;
    let listener = ServerListener::bind(&config.paths.socket)?;
    // Own the socket file as soon as bind succeeds so later startup failures do not leave stale sockets.
    let _files_guard = ServerFilesGuard {
        paths: config.paths.clone(),
    };
    secure_socket_file(&config.paths.socket)?;
    fs::write(&config.paths.pid, std::process::id().to_string()).context("failed to write muxr server pid")?;
    let initial_size = TerminalSize::new(80, 24)?;
    let mut runtime = SessionRuntime::spawn(config, &initial_size)?;
    runtime.persist_metadata(&config.paths)?;
    let delete_sessions = Arc::new(DeleteSessions::default());
    let (handshake_sender, mut handshake_receiver) = tokio::sync::mpsc::channel(CLIENT_HANDSHAKE_CHANNEL_LIMIT);
    let (attached_client_task_sender, mut attached_client_task_receiver) = tokio::sync::mpsc::channel(1);
    let handshake_context = SessionHandshakeContext {
        attached_client_task_sender: &attached_client_task_sender,
        config,
        delete_sessions: &delete_sessions,
    };
    let mut accepted_connections = 0_usize;
    let mut accepting_connections = true;
    let mut handles = Vec::new();

    let shutdown_message = loop {
        self::drain_session_runtime_messages(
            &mut runtime,
            &handshake_context,
            &mut attached_client_task_receiver,
            &mut handshake_receiver,
            &mut handles,
        )
        .await?;

        if delete_sessions.is_requested() {
            break SessionRuntimeShutdownMessage::DeleteSessionRequested;
        }

        if matches!(runtime.reap_exited_panes(config)?, ReapResult::Final) {
            break SessionRuntimeShutdownMessage::FinalPaneExited;
        }
        if runtime.pane_runtime_set_empty() {
            break SessionRuntimeShutdownMessage::PaneRuntimeSetEmpty;
        }

        crate::attached_client::join_finished_client_tasks(&mut handles).await?;
        // A handshake task can finish between the first drain and task join; drain again before honoring the
        // accepted-connection limit so the queued handshake can spawn its attached client.
        self::drain_session_runtime_messages(
            &mut runtime,
            &handshake_context,
            &mut attached_client_task_receiver,
            &mut handshake_receiver,
            &mut handles,
        )
        .await?;
        if !accepting_connections && handles.is_empty() {
            break SessionRuntimeShutdownMessage::AcceptedConnectionLimitReached;
        }

        tokio::select! {
            biased;
            message = attached_client_task_receiver.recv() => {
                if let Some(message) = message {
                    runtime.handle_attached_client_task_message(message);
                }
            }
            handshake = handshake_receiver.recv() => {
                if let Some(handshake) = handshake {
                    self::handle_session_handshake(
                        &mut runtime,
                        &handshake_context,
                        handshake,
                        &mut handles,
                    ).await?;
                }
            }
            accepted = listener.accept(), if accepting_connections => {
                self::handle_accepted_connection(
                    config,
                    accepted?,
                    &handshake_sender,
                    &mut accepted_connections,
                    &mut accepting_connections,
                    &mut handles,
                )?;
            }
            () = tokio::time::sleep(ACCEPT_POLL_INTERVAL) => {}
        }
    };

    // Shutdown cancels only pre-attach handshakes; while the loop is live they still use bounded backpressure.
    handshake_receiver.close();
    crate::attached_client::join_client_tasks(handles).await?;
    while let Ok(message) = attached_client_task_receiver.try_recv() {
        runtime.handle_attached_client_task_message(message);
    }
    if matches!(shutdown_message, SessionRuntimeShutdownMessage::DeleteSessionRequested)
        || delete_sessions.is_requested()
    {
        drop(runtime);
        crate::sessions_delete::remove_session_files(&config.paths)?;
    }
    Ok(())
}

fn handle_accepted_connection(
    config: &ServerConfig,
    connection: ServerConnection,
    handshake_sender: &tokio::sync::mpsc::Sender<SessionClientHandshake>,
    accepted_connections: &mut usize,
    accepting_connections: &mut bool,
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) -> rootcause::Result<()> {
    *accepted_connections = accepted_connections
        .checked_add(1)
        .ok_or_else(|| report!("muxr accepted connection count overflowed"))?;
    crate::attached_client::spawn_client_handshake_task(connection, handshake_sender, handles);

    if let Some(max_accepted_connections) = config.max_accepted_connections
        && *accepted_connections >= max_accepted_connections
    {
        *accepting_connections = false;
    }
    Ok(())
}

async fn drain_session_runtime_messages(
    runtime: &mut SessionRuntime,
    context: &SessionHandshakeContext<'_>,
    attached_client_task_receiver: &mut tokio::sync::mpsc::Receiver<SessionAttachedClientTaskMessage>,
    handshake_receiver: &mut tokio::sync::mpsc::Receiver<SessionClientHandshake>,
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) -> rootcause::Result<()> {
    while let Ok(message) = attached_client_task_receiver.try_recv() {
        runtime.handle_attached_client_task_message(message);
    }
    while let Ok(handshake) = handshake_receiver.try_recv() {
        self::handle_session_handshake(runtime, context, handshake, handles).await?;
    }
    Ok(())
}

async fn handle_session_handshake(
    runtime: &mut SessionRuntime,
    context: &SessionHandshakeContext<'_>,
    handshake: SessionClientHandshake,
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) -> rootcause::Result<()> {
    let SessionClientHandshake {
        mut connection,
        message,
    } = handshake;
    match runtime.handle_handshake_message(context.config, message) {
        SessionHandshakeOutcome::AttachAccepted(attach_request) => {
            let attached_client_task_runtime = runtime.attached_client_task_runtime(
                context.attached_client_task_sender.clone(),
                Arc::clone(context.delete_sessions),
            )?;
            crate::attached_client::spawn_attached_client_task(
                context.config,
                attached_client_task_runtime,
                connection,
                attach_request,
                handles,
            );
        }
        SessionHandshakeOutcome::DeleteSessionRequested => {
            crate::sessions_delete::handle_handshake_delete(
                &mut connection,
                context.delete_sessions.as_ref(),
                context.config.client_write_timeout,
            )
            .await?;
        }
        SessionHandshakeOutcome::NoClient => {}
        SessionHandshakeOutcome::Respond(event) => {
            let _sent = crate::attached_client::send_connection_event_with_timeout(
                &mut connection,
                &event,
                context.config.client_write_timeout,
            )
            .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub mod test_helpers {
    use std::path::Path;
    use std::time::Duration;

    use muxr_config::MuxrConfig;
    use muxr_core::SessionName;
    use muxr_core::SessionPaths;

    use super::ServerConfig;
    use crate::pty::ShellCmd;

    const TEST_CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
    const TEST_CLIENT_HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(500);
    const TEST_CLIENT_WRITE_TIMEOUT: Duration = Duration::from_millis(500);

    pub fn session_paths(base: &Path, raw: &str) -> rootcause::Result<(SessionName, SessionPaths)> {
        let session = raw.parse()?;
        let state_root = base.join("muxr");
        let root = state_root.join("sessions").join(raw);

        Ok((
            session,
            SessionPaths {
                socket: state_root.join("s").join(format!("{raw}.sock")),
                pid: root.join("server.pid"),
                layout: root.join("layout.json"),
                panes: root.join("panes"),
                root,
            },
        ))
    }

    pub fn shell_cmd(program: &str) -> ShellCmd {
        ShellCmd::new(program).expect("test shell cmd must have a nonempty program path")
    }

    pub fn shell_cmd_with_args(program: &str, args: &[&str]) -> ShellCmd {
        ShellCmd::with_args(program, args.iter().copied()).expect("test shell cmd must have valid args")
    }

    pub fn server_config(base: &Path, raw: &str) -> rootcause::Result<ServerConfig> {
        let (session, paths) = self::session_paths(base, raw)?;
        Ok(ServerConfig {
            client_heartbeat_interval: TEST_CLIENT_HEARTBEAT_INTERVAL,
            client_heartbeat_timeout: TEST_CLIENT_HEARTBEAT_TIMEOUT,
            client_write_timeout: TEST_CLIENT_WRITE_TIMEOUT,
            external_layout: None,
            user_config: std::sync::Arc::new(MuxrConfig::default()),
            session,
            paths,
            max_accepted_connections: None,
            shell_cmd: self::shell_cmd("/bin/sh"),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::thread;
    use std::time::Instant;

    use muxr_core::AttachRequest;
    use muxr_core::ClientKey;
    use muxr_core::ClientKeyCode;
    use muxr_core::ClientKeyModifiers;
    use muxr_core::ClientMouseEvent;
    use muxr_core::ClientMousePosition;
    use muxr_core::ClientRequest;
    use muxr_core::LayoutSnapshot;
    use muxr_core::PaneId;
    use muxr_core::PaneRegionSnapshot;
    use muxr_core::PaneRegionsSnapshot;
    use muxr_core::PaneScrollDirection;
    use muxr_core::RenderCell;
    use muxr_core::RenderRowSpan;
    use muxr_core::RenderUpdate;
    use muxr_core::ServerError;
    use muxr_core::ServerEvent;
    use muxr_transport::ClientConnection;
    use muxr_transport::ClientEventReader;
    use muxr_transport::ClientRequestWriter;

    use super::test_helpers::server_config;
    use super::test_helpers::session_paths;
    use super::test_helpers::shell_cmd;
    use super::test_helpers::shell_cmd_with_args;
    use super::*;
    use crate::pane_close::ClosePaneOutcome;
    use crate::pane_runtime::PaneRuntimes;
    use crate::pane_split::PaneSplitAxis;
    use crate::session_files::PRIVATE_SOCKET_MODE;
    use crate::session_runtime::initial_attached_render;
    use crate::session_runtime::resize_panes_to_layout;
    use crate::session_start_seed::SessionStartSeed;

    const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(2);
    const TEST_CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
    const TEST_CLIENT_HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(500);
    const TEST_CLIENT_WRITE_TIMEOUT: Duration = Duration::from_millis(500);

    type PaneRegionTuple = (String, u16, u16, u16, u16);

    struct AttachedTestClient {
        layout: LayoutSnapshot,
        pane_regions: PaneRegionsSnapshot,
        reader: ClientEventReader,
        writer: ClientRequestWriter,
    }

    #[test]
    fn test_serve_when_started_creates_session_root_socket_and_pid() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            self::make_public_session_dirs(&paths)?;
            let handle = self::spawn_test_server(&session, &paths, 1);

            self::wait_for_socket(&paths.socket)?;
            self::wait_for_path(&paths.layout)?;

            assert2::assert!(paths.root.is_dir());
            assert2::assert!(paths.panes.is_dir());
            assert2::assert!(paths.layout.exists());
            assert2::assert!(paths.socket.exists());
            assert2::assert!(paths.pid.exists());
            self::assert_session_state_is_private(&paths)?;

            self::attach_and_detach(&session, &paths).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_disconnects_accepts_future_attach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            drop(self::open_attached_client(&session, &paths).await?);
            tokio::time::sleep(Duration::from_millis(25)).await;

            self::attach_and_detach(&session, &paths).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_reattached_accepts_second_attach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;

            self::attach_and_detach(&session, &paths).await?;
            self::attach_and_detach(&session, &paths).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_attached_reports_current_layout_snapshot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 1);

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::attach_request(&session)?).await?;
            let Some(ServerEvent::Attached(attached)) = connection.recv_event().await? else {
                return Err(report!("expected server attached response"));
            };

            pretty_assertions::assert_eq!(attached.layout.active_tab().to_string(), "tab-1");
            let Some(tab) = attached.layout.tabs().first() else {
                return Err(report!("expected one tab in layout snapshot"));
            };
            pretty_assertions::assert_eq!(tab.id().to_string(), "tab-1");
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-1");
            let Some(pane) = tab.panes().first() else {
                return Err(report!("expected one pane in layout snapshot"));
            };
            pretty_assertions::assert_eq!(pane.id.to_string(), "pane-1");

            connection.send_request(&ClientRequest::Detach).await?;
            self::read_connection_until_detached(&mut connection).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_second_client_attaches_rejects_it() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut first_client = self::open_attached_client(&session, &paths).await?;
            let mut second_client = self::connect_client(&paths).await?;

            second_client.send_request(&self::attach_request(&session)?).await?;
            let Some(ServerEvent::Error(error)) = second_client.recv_event().await? else {
                return Err(report!("expected second attach rejection"));
            };

            pretty_assertions::assert_eq!(error, ServerError::ClientAlreadyAttached);
            first_client.writer.send_request(&ClientRequest::Detach).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_never_sends_attach_request_does_not_occupy_attach_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let idle_client = self::connect_client(&paths).await?;

            self::attach_and_detach(&session, &paths).await?;

            drop(idle_client);
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_attached_client_does_not_answer_heartbeat_releases_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut stuck_client = self::connect_client(&paths).await?;
            stuck_client.send_request(&self::attach_request(&session)?).await?;
            tokio::time::sleep(
                TEST_CLIENT_HEARTBEAT_INTERVAL
                    + TEST_CLIENT_HEARTBEAT_TIMEOUT
                    + TEST_CLIENT_WRITE_TIMEOUT
                    + Duration::from_millis(100),
            )
            .await;

            let responsive_client = self::open_attached_client(&session, &paths).await?;
            self::detach_client(responsive_client).await?;

            drop(stuck_client);
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_ping_is_first_request_returns_pong_without_claiming_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut probe = self::connect_client(&paths).await?;
            probe.send_request(&ClientRequest::Ping).await?;
            pretty_assertions::assert_eq!(probe.recv_event().await?, Some(ServerEvent::Pong));

            self::attach_and_detach(&session, &paths).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_delete_session_is_first_request_stops_server_and_removes_state() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            self::wait_for_path(&paths.layout)?;
            let mut delete_client = self::connect_client(&paths).await?;

            delete_client.send_request(&ClientRequest::DeleteSession).await?;
            pretty_assertions::assert_eq!(delete_client.recv_event().await?, Some(ServerEvent::Deleted));
            self::join_server_with_timeout(handle)?;

            assert2::assert!(!paths.root.exists());
            assert2::assert!(!paths.socket.exists());
            assert2::assert!(!paths.pid.exists());
            Ok(())
        })
    }

    #[test]
    fn test_serve_when_delete_session_arrives_while_client_is_attached_removes_state() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, None, self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let _attached_client = self::open_attached_client(&session, &paths).await?;
            let mut delete_client = self::connect_client(&paths).await?;

            delete_client.send_request(&ClientRequest::DeleteSession).await?;
            pretty_assertions::assert_eq!(delete_client.recv_event().await?, Some(ServerEvent::Deleted));
            self::join_server_with_timeout(handle)?;

            assert2::assert!(!paths.root.exists());
            assert2::assert!(!paths.socket.exists());
            assert2::assert!(!paths.pid.exists());
            Ok(())
        })
    }

    #[test]
    fn test_active_pane_session_metadata_when_cwd_was_synced_uses_active_pane_cwd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let active_cwd = tempfile::tempdir()?;
        let active_cwd = active_cwd.path().to_string_lossy().into_owned();
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.sync_terminal_titles(&[(PaneId::new(1)?, Some(active_cwd.clone()))]);

        let metadata = self::active_pane_session_metadata(&config, &layout)?;

        pretty_assertions::assert_eq!(metadata.cwd, active_cwd);
        Ok(())
    }

    #[test]
    fn test_layout_split_active_pane_when_cwd_was_synced_new_pane_inherits_active_pane_cwd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let active_cwd = tempfile::tempdir()?;
        let active_cwd = active_cwd.path().to_string_lossy().into_owned();
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.sync_terminal_titles(&[(PaneId::new(1)?, Some(active_cwd.clone()))]);

        let pane_id = layout.split_active_pane(
            config.user_config.layout,
            self::active_pane_session_metadata(&config, &layout)?,
            PaneSplitAxis::Vertical,
        )?;

        let pane = layout
            .pane(pane_id)
            .ok_or_else(|| report!("expected split pane to exist").attach(format!("pane_id={pane_id}")))?;
        pretty_assertions::assert_eq!(pane.cwd, active_cwd);
        Ok(())
    }

    #[test]
    fn test_layout_create_tab_when_cwd_was_synced_new_pane_inherits_active_pane_cwd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let active_cwd = tempfile::tempdir()?;
        let active_cwd = active_cwd.path().to_string_lossy().into_owned();
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.sync_terminal_titles(&[(PaneId::new(1)?, Some(active_cwd.clone()))]);

        let pane_id = layout.create_tab(self::active_pane_session_metadata(&config, &layout)?)?;

        let pane = layout
            .pane(pane_id)
            .ok_or_else(|| report!("expected tab pane to exist").attach(format!("pane_id={pane_id}")))?;
        pretty_assertions::assert_eq!(pane.cwd, active_cwd);
        Ok(())
    }

    #[test]
    fn test_handle_close_pane_cmd_when_title_cwd_is_pending_persists_synced_cwd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = self::server_config(tempdir.path(), "work")?;
        config.shell_cmd = self::shell_cmd("/bin/cat");
        fs::create_dir_all(&config.paths.root).context("failed to create muxr test session root")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        let terminal_size = TerminalSize::new(80, 24)?;
        let start_seed = SessionStartSeed {
            layout: layout.clone(),
            startup_cmds: Vec::new(),
        };
        let mut runtimes = PaneRuntimes::spawn_for_start_seed(&config, &start_seed, &terminal_size)?;
        let pane_id = PaneId::new(1)?;
        {
            let handle = runtimes.handle(pane_id)?;
            let _scrolled_to_bottom = handle.write_input(b"\x1b]2;~\x07\n")?;
        }

        let started_at = Instant::now();
        loop {
            let title = runtimes.handle(pane_id)?.terminal_title()?;
            if title.as_deref() == Some("~") {
                break;
            }
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr test terminal title"));
            }
            thread::sleep(Duration::from_millis(10));
        }

        let outcome = crate::pane_close::handle_close_pane_cmd(&config, &mut layout, &mut runtimes)?;

        pretty_assertions::assert_eq!(outcome, ClosePaneOutcome::Final { pane_id });
        let persisted = crate::state::persisted::load_metadata(&config.paths, &config.session)?
            .ok_or_else(|| report!("expected muxr layout metadata"))?;
        let pane = persisted
            .pane(pane_id)
            .ok_or_else(|| report!("expected persisted muxr pane").attach(format!("pane_id={pane_id}")))?;
        pretty_assertions::assert_eq!(pane.cwd, "~");
        Ok(())
    }

    #[test]
    fn test_initial_attached_render_when_detached_output_arrived_does_not_mark_attention() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = self::server_config(tempdir.path(), "work")?;
        config.shell_cmd = self::shell_cmd_with_args("/bin/sh", &["-c", "printf dirty; sleep 30"]);
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(
            config.user_config.layout,
            self::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        let start_seed = SessionStartSeed {
            layout: layout.clone(),
            startup_cmds: Vec::new(),
        };
        let runtimes = PaneRuntimes::spawn_for_start_seed(&config, &start_seed, &terminal_size)?;
        let inactive_pane = PaneId::new(1)?;
        let active_pane = PaneId::new(2)?;
        self::wait_for_runtime_snapshot_contains(&runtimes, inactive_pane, "dirty")?;
        self::wait_for_runtime_snapshot_contains(&runtimes, active_pane, "dirty")?;

        self::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        drop(self::initial_attached_render(
            &config,
            &mut layout,
            &runtimes,
            &crate::pane_tracked_process::PaneTrackedProcesses::default(),
            &terminal_size,
        )?);

        pretty_assertions::assert_eq!(layout.attention_pane_ids(), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_serve_when_key_request_arrives_writes_raw_bytes_and_stays_attached() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('x'),
                    modifiers: ClientKeyModifiers::NONE,
                    raw_bytes: b"x\n".to_vec(),
                }))
                .await?;

            self::read_until_render_contains(&mut client, b"x").await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_scrollback_editor_key_arrives_opens_dump_and_restores_layout() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let mut user_config = MuxrConfig::default();
            user_config.scrollback.editor = muxr_config::ScrollbackEditorConfig {
                program: "/bin/sh",
                args: &["-c", "cat \"$1\"; sleep 30", "muxr-test-scrollback-editor"],
            };
            let handle = self::spawn_test_server_with_user_config(
                &session,
                &paths,
                Some(1),
                self::shell_cmd("/bin/cat"),
                user_config,
            );

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Input(b"before-editor\n".to_vec()))
                .await
                .context("failed to send pre-editor input")?;
            self::read_until_render_contains(&mut client, b"before-editor").await?;

            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('S'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bS".to_vec(),
                }))
                .await
                .context("failed to send open scrollback editor key")?;
            let editor_layout = self::read_until_layout(&mut client).await?;
            let editor_tab = editor_layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected scrollback editor tab"))?;
            pretty_assertions::assert_eq!(editor_tab.active_pane().to_string(), "pane-2");
            self::read_until_render_contains(&mut client, b"before-editor").await?;

            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('W'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bW".to_vec(),
                }))
                .await
                .context("failed to send close scrollback editor key")?;
            let restored_layout = self::read_until_layout(&mut client).await?;
            let restored_tab = restored_layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected restored tab"))?;
            pretty_assertions::assert_eq!(restored_tab.active_pane().to_string(), "pane-1");
            self::assert_layout_metadata_panes(&paths, &[1], 1)?;
            assert2::assert!(!paths.panes.join("2").exists());

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_scrollback_editor_is_open_focus_pane_at_keeps_editor_active() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let mut user_config = MuxrConfig::default();
            user_config.scrollback.editor = muxr_config::ScrollbackEditorConfig {
                program: "/bin/sh",
                args: &[
                    "-c",
                    "cat \"$1\"; exec sed 's/^/editor:/'",
                    "muxr-test-scrollback-editor",
                ],
            };
            let handle = self::spawn_test_server_with_user_config(
                &session,
                &paths,
                Some(1),
                self::shell_cmd("/bin/cat"),
                user_config,
            );

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            let split_layout = self::read_until_layout(&mut client).await?;
            let split_tab = split_layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected split tab before scrollback editor"))?;
            pretty_assertions::assert_eq!(split_tab.active_pane().to_string(), "pane-2");

            client
                .writer
                .send_request(&ClientRequest::Input(b"before-editor\n".to_vec()))
                .await?;
            self::read_until_render_contains(&mut client, b"before-editor").await?;

            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('S'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bS".to_vec(),
                }))
                .await?;
            let editor_layout = self::read_until_layout(&mut client).await?;
            let editor_tab = editor_layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected scrollback editor tab"))?;
            pretty_assertions::assert_eq!(editor_tab.active_pane().to_string(), "pane-3");
            self::read_until_render_contains(&mut client, b"before-editor").await?;
            let editor_regions =
                self::read_until_pane_regions_matching(&mut client, "editor layout includes sibling pane", |regions| {
                    Ok(regions
                        .regions()
                        .iter()
                        .any(|region| region.id().to_string() == "pane-1")
                        && regions
                            .regions()
                            .iter()
                            .any(|region| region.id().to_string() == "pane-3"))
                })
                .await?;

            client
                .writer
                .send_request(&ClientRequest::FocusPaneAt(self::pane_position(
                    &editor_regions,
                    "pane-1",
                )?))
                .await?;
            client
                .writer
                .send_request(&ClientRequest::Input(b"after-focus\n".to_vec()))
                .await?;
            self::read_until_render_contains(&mut client, b"editor:after-focus").await?;

            drop(client);
            self::join_server_with_timeout(handle)?;
            self::assert_layout_metadata_panes(&paths, &[1, 2], 2)?;
            assert2::assert!(!paths.panes.join("3").exists());
            Ok(())
        })
    }

    #[test]
    fn test_serve_when_terminal_title_reports_cwd_persists_metadata() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Input(b"\x1b]2;~\x07\n".to_vec()))
                .await?;

            let layout = self::read_until_sidebar_layout(&mut client).await?;
            let Some(tab) = layout.tabs().first() else {
                return Err(report!("expected muxr test layout tab"));
            };
            let Some(pane) = tab.panes().first() else {
                return Err(report!("expected muxr test layout pane"));
            };
            pretty_assertions::assert_eq!(pane.cwd, "~");
            pretty_assertions::assert_eq!(pane.cmd_label, None);

            let persisted = crate::state::persisted::load_metadata(&paths, &session)?
                .ok_or_else(|| report!("expected muxr layout metadata"))?;
            let pane = persisted
                .pane(PaneId::new(1)?)
                .ok_or_else(|| report!("expected persisted muxr pane"))?;
            pretty_assertions::assert_eq!(pane.cwd, "~");

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_create_tab_key_arrives_sends_layout_and_persists_metadata() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('E'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bE".to_vec(),
                }))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            pretty_assertions::assert_eq!(layout.active_tab().to_string(), "tab-2");
            pretty_assertions::assert_eq!(
                layout.tabs().iter().map(|tab| tab.id().to_string()).collect::<Vec<_>>(),
                vec!["tab-1", "tab-2"],
            );
            self::assert_layout_metadata_tabs(&paths, &[1, 2], 2)?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_layout_metadata_exists_restores_tab_order_on_attach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let config = self::server_config(tempdir.path(), "work")?;
            fs::create_dir_all(&config.paths.root)?;
            let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
            layout.create_tab(self::metadata("sh", 2))?;
            crate::state::persisted::write_metadata(&config.paths, &layout)?;
            let paths = config.paths.clone();
            let session = config.session.clone();
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let client = self::open_attached_client(&session, &paths).await?;
            pretty_assertions::assert_eq!(client.layout.active_tab().to_string(), "tab-2");
            pretty_assertions::assert_eq!(
                client
                    .layout
                    .tabs()
                    .iter()
                    .map(|tab| tab.id().to_string())
                    .collect::<Vec<_>>(),
                vec!["tab-1", "tab-2"],
            );
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_split_pane_key_arrives_sends_layout_and_routes_input_to_new_pane() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected tab after split"))?;
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-2");
            pretty_assertions::assert_eq!(
                tab.panes().iter().map(|pane| pane.id.to_string()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );

            client
                .writer
                .send_request(&ClientRequest::Input(b"new-pane\n".to_vec()))
                .await?;
            self::read_until_render_contains(&mut client, b"new-pane").await?;
            self::assert_layout_metadata_panes(&paths, &[1, 2], 2)?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_no_button_mouse_motion_arrives_does_not_focus_hovered_pane() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            drop(self::read_until_layout(&mut client).await?);

            client
                .writer
                .send_request(&ClientRequest::Mouse(ClientMouseEvent {
                    button: 35,
                    phase: muxr_core::ClientMouseEventPhase::Press,
                    position: muxr_core::ClientMousePosition { row: 0, col: 0 },
                }))
                .await?;
            client
                .writer
                .send_request(&ClientRequest::Input(b"still-pane-2\n".to_vec()))
                .await?;

            self::read_until_render_contains(&mut client, b"still-pane-2").await?;
            self::assert_layout_metadata_panes(&paths, &[1, 2], 2)?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_wheel_over_inactive_plain_pane_scrolls_pointed_history_without_focus_change()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd_with_args(
                    "/bin/sh",
                    &[
                        "-c",
                        "i=0; while [ $i -lt 80 ]; do printf 'line-%02d\\n' \"$i\"; i=$((i + 1)); done; sleep 30",
                    ],
                ),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut client, b"line-79").await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected tab after split"))?;
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-2");
            self::read_until_render_contains(&mut client, b"line-79").await?;
            let ready_regions =
                self::read_until_pane_regions_matching(&mut client, "both panes have scrollback", |regions| {
                    Ok(self::pane_region(regions, "pane-1")?.visible_top_row() > 0
                        && self::pane_region(regions, "pane-2")?.visible_top_row() > 0)
                })
                .await?;
            let pane_1_before = self::pane_region(&ready_regions, "pane-1")?.visible_top_row();
            let pane_2_before = self::pane_region(&ready_regions, "pane-2")?.visible_top_row();

            client
                .writer
                .send_request(&ClientRequest::Mouse(ClientMouseEvent {
                    button: 64,
                    phase: muxr_core::ClientMouseEventPhase::Press,
                    position: self::pane_position(&ready_regions, "pane-1")?,
                }))
                .await?;

            let scrolled_regions =
                self::read_until_pane_regions_matching(&mut client, "pointed pane scrollback moved", |regions| {
                    Ok(self::pane_region(regions, "pane-1")?.visible_top_row() < pane_1_before
                        && self::pane_region(regions, "pane-2")?.visible_top_row() == pane_2_before)
                })
                .await?;
            pretty_assertions::assert_eq!(
                self::pane_region(&scrolled_regions, "pane-2")?.visible_top_row(),
                pane_2_before,
            );
            self::assert_layout_metadata_panes(&paths, &[1, 2], 2)?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_selection_line_scroll_cannot_move_sends_noop_scroll_result() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            let position = ClientMousePosition { row: 0, col: 0 };
            client
                .writer
                .send_request(&ClientRequest::ScrollPaneLineAt {
                    position,
                    direction: PaneScrollDirection::Down,
                })
                .await?;

            pretty_assertions::assert_eq!(
                self::read_until_scroll_pane_line_result(&mut client).await?,
                (position, PaneScrollDirection::Down, false),
            );
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_wheel_over_app_mouse_pane_forwards_mouse_bytes_to_pty() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd_with_args(
                    "/bin/sh",
                    &[
                        "-c",
                        "printf '\\033[?1002h\\033[?1006hready\\n'; \
                         stty raw -echo; \
                         dd bs=1 count=10 2>/dev/null | od -An -tx1 -v; \
                         sleep 30",
                    ],
                ),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut client, b"ready").await?;
            let regions = self::read_until_pane_regions_matching(&mut client, "app mouse mode enabled", |regions| {
                Ok(self::pane_region(regions, "pane-1")?.mouse_mode() == muxr_core::PaneMouseMode::ButtonMotion)
            })
            .await?;

            client
                .writer
                .send_request(&ClientRequest::Mouse(ClientMouseEvent {
                    button: 64,
                    phase: muxr_core::ClientMouseEventPhase::Press,
                    position: self::pane_position(&regions, "pane-1")?,
                }))
                .await?;

            self::read_until_render_contains_hex_bytes(
                &mut client,
                &["1b", "5b", "3c", "36", "34", "3b", "31", "3b", "31", "4d"],
            )
            .await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_wheel_over_alternate_screen_without_mouse_protocol_sends_faux_scroll_input()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd_with_args(
                    "/bin/sh",
                    &[
                        "-c",
                        "printf '\\033[?1049hready\\n'; \
                         stty raw -echo; \
                         dd bs=1 count=9 2>/dev/null | od -An -tx1 -v; \
                         sleep 30",
                    ],
                ),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut client, b"ready").await?;

            client
                .writer
                .send_request(&ClientRequest::Mouse(ClientMouseEvent {
                    button: 64,
                    phase: muxr_core::ClientMouseEventPhase::Press,
                    position: self::pane_position(&client.pane_regions, "pane-1")?,
                }))
                .await?;

            self::read_until_render_contains_hex_bytes(
                &mut client,
                &["1b", "5b", "41", "1b", "5b", "41", "1b", "5b", "41"],
            )
            .await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_close_pane_key_arrives_removes_active_pane_and_keeps_remaining_pty() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            drop(self::read_until_layout(&mut client).await?);

            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('W'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bW".to_vec(),
                }))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected tab after close"))?;
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-1");
            pretty_assertions::assert_eq!(
                tab.panes().iter().map(|pane| pane.id.to_string()).collect::<Vec<_>>(),
                vec!["pane-1"],
            );

            client
                .writer
                .send_request(&ClientRequest::Input(b"remaining\n".to_vec()))
                .await?;
            self::read_until_render_contains(&mut client, b"remaining").await?;
            self::assert_layout_metadata_panes(&paths, &[1], 1)?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_final_pane_is_closed_persists_and_exits() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

        self::runtime()?.block_on(async {
            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('W'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bW".to_vec(),
                }))
                .await?;
            self::read_client_until_detached(&mut client).await?;
            drop(client);
            Ok::<(), rootcause::Report>(())
        })?;

        self::join_server_with_timeout(handle)?;
        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        self::assert_final_closed_layout_metadata(&paths)?;
        Ok(())
    }

    #[test]
    fn test_serve_resize_mode_sequence_resizes_and_escape_exits_mode() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));
            let size = TerminalSize::new(80, 24)?;

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            drop(self::read_until_layout(&mut client).await?);
            let before_resize = crate::state::persisted::load_metadata(&paths, &session)?
                .ok_or_else(|| report!("expected muxr layout metadata to load before resize"))?;
            let before_regions = self::layout_active_tab_pane_regions(&before_resize, &size)?;
            pretty_assertions::assert_eq!(
                before_regions.iter().map(|(id, ..)| id.as_str()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('R'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bR".to_vec(),
                }))
                .await?;
            drop(self::read_until_layout(&mut client).await?);
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('h'),
                    modifiers: ClientKeyModifiers::NONE,
                    raw_bytes: b"h".to_vec(),
                }))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected tab after resize"))?;
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-2");
            pretty_assertions::assert_eq!(
                tab.panes().iter().map(|pane| pane.id.to_string()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );
            let persisted = crate::state::persisted::load_metadata(&paths, &session)?
                .ok_or_else(|| report!("expected muxr layout metadata to load"))?;
            let after_regions = self::layout_active_tab_pane_regions(&persisted, &size)?;
            pretty_assertions::assert_eq!(
                after_regions.iter().map(|(id, ..)| id.as_str()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );
            let before_first = &before_regions[0];
            let before_second = &before_regions[1];
            let after_first = &after_regions[0];
            let after_second = &after_regions[1];
            pretty_assertions::assert_eq!(
                (after_first.1, after_first.2, after_first.4),
                (before_first.1, before_first.2, before_first.4),
            );
            pretty_assertions::assert_eq!((after_second.2, after_second.4), (before_second.2, before_second.4));
            assert2::assert!(after_first.3 < before_first.3);
            assert2::assert!(after_second.3 > before_second.3);
            pretty_assertions::assert_eq!(after_second.1, after_first.1 + after_first.3 + 1);
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Esc,
                    modifiers: ClientKeyModifiers::NONE,
                    raw_bytes: b"\x1b".to_vec(),
                }))
                .await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('x'),
                    modifiers: ClientKeyModifiers::NONE,
                    raw_bytes: b"x\n".to_vec(),
                }))
                .await?;
            self::read_until_render_contains(&mut client, b"x").await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_shell_outputs_while_detached_replays_output_on_reattach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(2),
                self::shell_cmd_with_args("/bin/sh", &["-c", "printf first; sleep 1; printf second; sleep 30"]),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut first_client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut first_client, b"first").await?;
            self::detach_client(first_client).await?;

            tokio::time::sleep(Duration::from_millis(1200)).await;

            let mut second_client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut second_client, b"second").await?;
            self::detach_client(second_client).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_floods_input_still_sends_output() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd_with_args("/bin/sh", &["-c", "sleep 0.1; printf ready; sleep 30"]),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::attach_request(&session)?).await?;
            let Some(ServerEvent::Attached(_)) = connection.recv_event().await? else {
                return Err(report!("expected server attached response"));
            };
            let (mut reader, mut writer) = connection.split();
            let flood_handle = tokio::spawn(async move {
                loop {
                    if writer.send_request(&ClientRequest::Input(Vec::new())).await.is_err() {
                        break;
                    }
                }
            });

            let read_result = self::read_reader_until_render_contains(&mut reader, b"ready").await;
            drop(reader);
            flood_handle.abort();
            drop(flood_handle.await);
            let join_result = self::join_server_with_timeout(handle);

            read_result?;
            join_result
        })
    }

    #[test]
    fn test_serve_when_shell_floods_output_still_detaches() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd_with_args("/bin/sh", &["-c", "while :; do printf x; done"]),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::attach_request(&session)?).await?;
            let Some(ServerEvent::Attached(_)) = connection.recv_event().await? else {
                return Err(report!("expected server attached response"));
            };
            self::read_connection_until_render_contains(&mut connection, b"x").await?;
            connection.send_request(&ClientRequest::Detach).await?;
            self::read_connection_until_detached(&mut connection).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_shell_exits_removes_socket_and_pid() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(
            &session,
            &paths,
            None,
            self::shell_cmd_with_args("/bin/sh", &["-c", "printf done"]),
        );

        self::wait_for_socket(&paths.socket)?;
        self::join_server_with_timeout(handle)?;

        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        self::assert_final_layout_metadata(&paths, 0, true)?;
        Ok(())
    }

    #[test]
    fn test_serve_when_shell_exits_with_error_persists_exit_status() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(
            &session,
            &paths,
            None,
            self::shell_cmd_with_args("/bin/sh", &["-c", "exit 7"]),
        );

        self::wait_for_socket(&paths.socket)?;
        self::join_server_with_timeout(handle)?;

        self::assert_final_layout_metadata(&paths, 7, false)?;
        Ok(())
    }

    #[test]
    fn test_serve_when_startup_fails_after_bind_removes_socket_and_pid() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;

        let config = ServerConfig {
            client_heartbeat_interval: TEST_CLIENT_HEARTBEAT_INTERVAL,
            client_heartbeat_timeout: TEST_CLIENT_HEARTBEAT_TIMEOUT,
            client_write_timeout: TEST_CLIENT_WRITE_TIMEOUT,
            external_layout: None,
            user_config: Arc::new(MuxrConfig::default()),
            session,
            paths: paths.clone(),
            max_accepted_connections: None,
            shell_cmd: self::shell_cmd("/bin/muxr-missing-shell"),
        };
        let result = tokio::runtime::Runtime::new()
            .context("failed to build muxr tokio runtime")?
            .block_on(self::serve_async(&config));

        assert2::assert!(result.is_err());
        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        Ok(())
    }

    fn spawn_test_server(
        session: &SessionName,
        paths: &SessionPaths,
        max_accepted_connections: usize,
    ) -> thread::JoinHandle<rootcause::Result<()>> {
        self::spawn_test_server_with_shell(
            session,
            paths,
            Some(max_accepted_connections),
            self::shell_cmd_with_args("/bin/sh", &["-c", "sleep 30"]),
        )
    }

    fn spawn_test_server_with_shell(
        session: &SessionName,
        paths: &SessionPaths,
        max_accepted_connections: Option<usize>,
        shell_cmd: ShellCmd,
    ) -> thread::JoinHandle<rootcause::Result<()>> {
        self::spawn_test_server_with_user_config(
            session,
            paths,
            max_accepted_connections,
            shell_cmd,
            MuxrConfig::default(),
        )
    }

    fn spawn_test_server_with_user_config(
        session: &SessionName,
        paths: &SessionPaths,
        max_accepted_connections: Option<usize>,
        shell_cmd: ShellCmd,
        user_config: MuxrConfig,
    ) -> thread::JoinHandle<rootcause::Result<()>> {
        thread::spawn({
            let session = session.clone();
            let paths = paths.clone();
            move || {
                let config = ServerConfig {
                    client_heartbeat_interval: TEST_CLIENT_HEARTBEAT_INTERVAL,
                    client_heartbeat_timeout: TEST_CLIENT_HEARTBEAT_TIMEOUT,
                    client_write_timeout: TEST_CLIENT_WRITE_TIMEOUT,
                    external_layout: None,
                    user_config: Arc::new(user_config),
                    session,
                    paths,
                    max_accepted_connections,
                    shell_cmd,
                };
                tokio::runtime::Runtime::new()
                    .context("failed to build muxr tokio runtime")?
                    .block_on(self::serve_async(&config))
            }
        })
    }

    async fn connect_client(paths: &SessionPaths) -> rootcause::Result<ClientConnection> {
        let started_at = Instant::now();

        loop {
            match ClientConnection::connect(&paths.socket).await {
                Ok(connection) => return Ok(connection),
                Err(error) => {
                    if started_at.elapsed() > SERVER_READY_TIMEOUT {
                        return Err(error);
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn open_attached_client(
        session: &SessionName,
        paths: &SessionPaths,
    ) -> rootcause::Result<AttachedTestClient> {
        let mut connection = self::connect_client(paths).await?;

        connection.send_request(&self::attach_request(session)?).await?;
        let event = connection.recv_event().await?;
        let Some(ServerEvent::Attached(attached)) = event else {
            return Err(report!("expected server attached response").attach(format!("{event:?}")));
        };
        let layout = attached.layout;
        let pane_regions = attached.pane_regions;
        let (reader, writer) = connection.split();

        Ok(AttachedTestClient {
            layout,
            pane_regions,
            reader,
            writer,
        })
    }

    async fn attach_and_detach(session: &SessionName, paths: &SessionPaths) -> rootcause::Result<()> {
        let client = self::open_attached_client(session, paths).await?;

        self::detach_client(client).await?;
        Ok(())
    }

    async fn detach_client(mut client: AttachedTestClient) -> rootcause::Result<()> {
        client.writer.send_request(&ClientRequest::Detach).await?;
        self::read_client_until_detached(&mut client).await
    }

    async fn read_client_until_detached(client: &mut AttachedTestClient) -> rootcause::Result<()> {
        loop {
            match client.reader.recv_event().await? {
                Some(ServerEvent::Detached) => break,
                Some(ServerEvent::Ping) => client.writer.send_request(&ClientRequest::Pong).await?,
                Some(
                    ServerEvent::Attached(_)
                    | ServerEvent::Pong
                    | ServerEvent::Layout(_)
                    | ServerEvent::SidebarLayout(_)
                    | ServerEvent::PaneRegions(_)
                    | ServerEvent::Render(_),
                ) => {}
                Some(event) => return Err(report!("expected detached event").attach(format!("{event:?}"))),
                None => return Err(report!("expected detached event")),
            }
        }
        Ok(())
    }

    fn join_server(handle: thread::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
        handle
            .join()
            .unwrap_or_else(|_| Err(report!("test muxr server thread panicked")))
    }

    fn join_server_with_timeout(handle: thread::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
        let started_at = Instant::now();
        while !handle.is_finished() {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr test server exit"));
            }

            thread::sleep(Duration::from_millis(10));
        }

        self::join_server(handle)
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }

    fn layout_active_tab_pane_regions(
        layout: &SessionLayout,
        size: &TerminalSize,
    ) -> rootcause::Result<Vec<PaneRegionTuple>> {
        Ok(layout
            .pane_regions(size)?
            .iter()
            .map(|region| {
                (
                    region.id.to_string(),
                    region.area.origin.col,
                    region.area.origin.row,
                    region.area.size.cols,
                    region.area.size.rows,
                )
            })
            .collect())
    }

    fn wait_for_runtime_snapshot_contains(
        runtimes: &PaneRuntimes,
        pane_id: PaneId,
        needle: &str,
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            let rendered = runtimes
                .handle(pane_id)?
                .render_snapshot()?
                .rows()
                .iter()
                .flat_map(|row| row.cells().iter().map(RenderCell::text))
                .collect::<String>();
            if rendered.contains(needle) {
                return Ok(());
            }
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr runtime snapshot").attach(rendered));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn make_public_session_dirs(paths: &SessionPaths) -> rootcause::Result<()> {
        let state_root = self::state_root(paths)?;
        fs::create_dir_all(state_root).context("failed to create public muxr test dir")?;
        fs::set_permissions(state_root, fs::Permissions::from_mode(0o755))
            .context("failed to set public muxr test dir permissions")?;
        Ok(())
    }

    fn assert_session_state_is_private(paths: &SessionPaths) -> rootcause::Result<()> {
        self::assert_mode(self::state_root(paths)?, crate::session_files::PRIVATE_DIR_MODE)?;
        self::assert_mode(&paths.socket, PRIVATE_SOCKET_MODE)?;
        Ok(())
    }

    fn state_root(paths: &SessionPaths) -> rootcause::Result<&Path> {
        let socket_root = self::parent_path(&paths.socket, "socket root")?;
        self::parent_path(socket_root, "state root")
    }

    fn parent_path<'a>(path: &'a Path, label: &str) -> rootcause::Result<&'a Path> {
        path.parent()
            .ok_or_else(|| report!("muxr test path has no parent").attach(format!("label={label}")))
    }

    fn assert_mode(path: &Path, expected_mode: u32) -> rootcause::Result<()> {
        let mode = fs::metadata(path)
            .context("failed to inspect muxr test path mode")?
            .permissions()
            .mode()
            & 0o777;

        pretty_assertions::assert_eq!(mode, expected_mode);
        Ok(())
    }

    fn wait_for_socket(path: &Path) -> rootcause::Result<()> {
        self::wait_for_path(path)
    }

    fn wait_for_path(path: &Path) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            if path.exists() {
                return Ok(());
            }

            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr test path").attach(path.display().to_string()));
            }

            thread::sleep(Duration::from_millis(10));
        }
    }

    fn attach_request(session: &SessionName) -> rootcause::Result<ClientRequest> {
        Ok(ClientRequest::Attach(AttachRequest {
            session: session.clone(),
            terminal_size: self::terminal_size()?,
        }))
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(80, 24)
    }

    async fn read_until_layout(client: &mut AttachedTestClient) -> rootcause::Result<LayoutSnapshot> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr layout update"));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Layout(layout) => {
                    client.layout = layout.clone();
                    return Ok(layout);
                }
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::Render(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_pane_regions_matching(
        client: &mut AttachedTestClient,
        description: &str,
        condition: impl Fn(&PaneRegionsSnapshot) -> rootcause::Result<bool>,
    ) -> rootcause::Result<PaneRegionsSnapshot> {
        if condition(&client.pane_regions)? {
            return Ok(client.pane_regions.clone());
        }
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr pane regions update").attach(description.to_owned()));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::PaneRegions(regions) => {
                    client.pane_regions = regions.clone();
                    if condition(&regions)? {
                        return Ok(regions);
                    }
                }
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::Render(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_scroll_pane_line_result(
        client: &mut AttachedTestClient,
    ) -> rootcause::Result<(ClientMousePosition, PaneScrollDirection, bool)> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr scroll line result"));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::ScrollPaneLineResult {
                    position,
                    direction,
                    scrolled,
                } => return Ok((position, direction, scrolled)),
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::Render(_)
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_sidebar_layout(client: &mut AttachedTestClient) -> rootcause::Result<LayoutSnapshot> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr sidebar layout update"));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::SidebarLayout(layout) => {
                    client.layout = layout.clone();
                    return Ok(layout);
                }
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::Render(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_render_contains(client: &mut AttachedTestClient, needle: &[u8]) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut rendered = String::new();
        let needle = String::from_utf8_lossy(needle);

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr rendered pty output").attach(rendered));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Render(update) => {
                    rendered.push_str(&self::render_update_text(&update));
                    if rendered.contains(needle.as_ref()) {
                        return Ok(());
                    }
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_render_contains_hex_bytes(
        client: &mut AttachedTestClient,
        expected: &[&str],
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut rendered = String::new();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr rendered hex bytes").attach(rendered));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Render(update) => {
                    rendered.push_str(&self::render_update_text(&update));
                    let tokens = rendered.split_whitespace().collect::<Vec<_>>();
                    if tokens.windows(expected.len()).any(|window| window == expected) {
                        return Ok(());
                    }
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_reader_until_render_contains(reader: &mut ClientEventReader, needle: &[u8]) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut rendered = String::new();
        let needle = String::from_utf8_lossy(needle);

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr rendered pty output").attach(rendered));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Render(update) => {
                    rendered.push_str(&self::render_update_text(&update));
                    if rendered.contains(needle.as_ref()) {
                        return Ok(());
                    }
                }
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Ping
                | ServerEvent::Pong
                | ServerEvent::Layout(_)
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::PaneRegions(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_connection_until_render_contains(
        connection: &mut ClientConnection,
        needle: &[u8],
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut rendered = String::new();
        let needle = String::from_utf8_lossy(needle);

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr rendered pty output").attach(rendered));
            }

            match tokio::time::timeout(Duration::from_millis(50), connection.recv_event()).await {
                Ok(Ok(Some(ServerEvent::Render(update)))) => {
                    rendered.push_str(&self::render_update_text(&update));
                    if rendered.contains(needle.as_ref()) {
                        return Ok(());
                    }
                }
                Ok(Ok(Some(ServerEvent::Ping))) => connection.send_request(&ClientRequest::Pong).await?,
                Ok(Ok(Some(ServerEvent::Error(error)))) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                Ok(Ok(
                    Some(
                        ServerEvent::Attached(_)
                        | ServerEvent::Deleted
                        | ServerEvent::Pong
                        | ServerEvent::Layout(_)
                        | ServerEvent::SidebarLayout(_)
                        | ServerEvent::PaneRegions(_)
                        | ServerEvent::ScrollPaneLineResult { .. }
                        | ServerEvent::Detached,
                    )
                    | None,
                ))
                | Err(_) => {}
                Ok(Err(error)) => return Err(error),
            }
        }
    }

    async fn read_connection_until_detached(connection: &mut ClientConnection) -> rootcause::Result<()> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr detach ack"));
            }

            match tokio::time::timeout(Duration::from_millis(50), connection.recv_event()).await {
                Ok(Ok(Some(ServerEvent::Detached))) => return Ok(()),
                Ok(Ok(Some(ServerEvent::Ping))) => connection.send_request(&ClientRequest::Pong).await?,
                Ok(Ok(Some(ServerEvent::Error(error)))) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                Ok(Ok(
                    Some(
                        ServerEvent::Attached(_)
                        | ServerEvent::Deleted
                        | ServerEvent::Pong
                        | ServerEvent::Layout(_)
                        | ServerEvent::SidebarLayout(_)
                        | ServerEvent::PaneRegions(_)
                        | ServerEvent::Render(_)
                        | ServerEvent::ScrollPaneLineResult { .. },
                    )
                    | None,
                ))
                | Err(_) => {}
                Ok(Err(error)) => return Err(error),
            }
        }
    }

    fn render_update_text(update: &RenderUpdate) -> String {
        match update {
            RenderUpdate::Baseline(baseline) => self::render_rows_text(baseline.rows()),
            RenderUpdate::Diff(diff) => self::render_rows_text(diff.rows()),
        }
    }

    fn render_rows_text(rows: &[RenderRowSpan]) -> String {
        rows.iter()
            .map(|row| row.cells().iter().map(RenderCell::text).collect::<String>())
            .collect()
    }

    fn pane_region<'a>(regions: &'a PaneRegionsSnapshot, pane_id: &str) -> rootcause::Result<&'a PaneRegionSnapshot> {
        regions
            .regions()
            .iter()
            .find(|region| region.id().to_string() == pane_id)
            .ok_or_else(|| report!("expected muxr test pane region").attach(format!("pane_id={pane_id}")))
    }

    fn pane_position(regions: &PaneRegionsSnapshot, pane_id: &str) -> rootcause::Result<ClientMousePosition> {
        let region = self::pane_region(regions, pane_id)?;
        Ok(ClientMousePosition {
            row: region.row(),
            col: region.col(),
        })
    }

    fn assert_final_layout_metadata(
        paths: &SessionPaths,
        expected_code: u64,
        expected_success: bool,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let pane = &layout["tabs"][0]["pane_tree"];

        pretty_assertions::assert_eq!(layout["version"].as_u64(), Some(u64::from(crate::state::VERSION)));
        pretty_assertions::assert_eq!(layout["session"].as_str(), Some("work"));
        pretty_assertions::assert_eq!(layout["active_tab"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(pane["id"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(pane["cmd_label"].as_str(), Some("sh"));
        assert2::assert!(pane["started_at"].as_u64().is_some());
        pretty_assertions::assert_eq!(pane["state"]["kind"].as_str(), Some("process_exited"));
        assert2::assert!(pane["state"]["at"].as_u64().is_some());
        pretty_assertions::assert_eq!(pane["state"]["status"]["code"].as_u64(), Some(expected_code));
        pretty_assertions::assert_eq!(pane["state"]["status"]["success"].as_bool(), Some(expected_success));
        Ok(())
    }

    fn assert_layout_metadata_tabs(
        paths: &SessionPaths,
        expected_tabs: &[u64],
        expected_active: u64,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let Some(tabs) = layout["tabs"].as_array() else {
            return Err(report!("muxr test layout metadata tabs are missing"));
        };
        let actual_tabs = tabs
            .iter()
            .map(|tab| {
                tab["id"]
                    .as_u64()
                    .ok_or_else(|| report!("muxr test layout metadata tab id is missing"))
            })
            .collect::<rootcause::Result<Vec<_>>>()?;

        pretty_assertions::assert_eq!(layout["active_tab"].as_u64(), Some(expected_active));
        pretty_assertions::assert_eq!(actual_tabs, expected_tabs.to_vec());
        Ok(())
    }

    fn assert_layout_metadata_panes(
        paths: &SessionPaths,
        expected_panes: &[u64],
        expected_active: u64,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let actual_panes = self::json_pane_tree_pane_ids(&layout["tabs"][0]["pane_tree"])?;

        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_u64(), Some(expected_active));
        pretty_assertions::assert_eq!(actual_panes, expected_panes.to_vec());
        Ok(())
    }

    fn assert_final_closed_layout_metadata(paths: &SessionPaths) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let pane = &layout["tabs"][0]["pane_tree"];

        pretty_assertions::assert_eq!(layout["active_tab"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(pane["id"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(pane["state"]["kind"].as_str(), Some("closed"));
        assert2::assert!(pane["state"]["at"].as_u64().is_some());
        assert2::assert!(pane["state"].get("status").is_none());
        Ok(())
    }

    fn json_pane_tree_pane_ids(node: &serde_json::Value) -> rootcause::Result<Vec<u64>> {
        let mut ids = Vec::new();
        self::collect_json_pane_tree_pane_ids(node, &mut ids)?;
        Ok(ids)
    }

    fn collect_json_pane_tree_pane_ids(node: &serde_json::Value, ids: &mut Vec<u64>) -> rootcause::Result<()> {
        match node["kind"].as_str() {
            Some("pane") => {
                let Some(id) = node["id"].as_u64() else {
                    return Err(report!("muxr test layout metadata pane id is missing"));
                };
                ids.push(id);
                Ok(())
            }
            Some("split") => {
                self::collect_json_pane_tree_pane_ids(&node["first"], ids)?;
                self::collect_json_pane_tree_pane_ids(&node["second"], ids)
            }
            Some(kind) => {
                Err(report!("muxr test layout metadata pane tree kind is invalid").attach(format!("kind={kind}")))
            }
            None => Err(report!("muxr test layout metadata pane tree kind is missing")),
        }
    }

    fn runtime() -> rootcause::Result<tokio::runtime::Runtime> {
        Ok(tokio::runtime::Runtime::new().context("failed to build muxr server test runtime")?)
    }
}
