use std::sync::Arc;

use muxr_core::AttachRequest;
use muxr_core::ClientRequest;
use muxr_core::PaneId;
use muxr_core::ServerError;
use muxr_core::ServerEvent;
use muxr_core::SessionPaths;
use muxr_core::TerminalSize;
use muxr_transport::ServerConnection;
use tokio::sync::mpsc::error::TrySendError;

use crate::pane::close::PaneExitOutcome;
use crate::pane::runtime::PaneRuntimeSetStatus;
use crate::pane::runtime::PaneRuntimes;
use crate::pty::PtyEvent;
use crate::server::ServerConfig;
use crate::session::delete::DeleteSessions;
use crate::session::start_seed::SessionStartSeed;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;

/// High-volume PTY output is bounded and coalesced: a full queue keeps one pending output-ready marker instead of
/// enqueueing every frame. Session runtime routing must preserve that behavior while keeping PTY state single-owned.
pub const PANE_OUTPUT_EVENT_CHANNEL_LIMIT: usize = 1024;
pub const CLIENT_HANDSHAKE_CHANNEL_LIMIT: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReapResult {
    Final,
    NoExitedPanes,
    Removed { pane_ids: Vec<PaneId> },
}

#[derive(Debug)]
pub enum SessionHandshakeMessage {
    AttachRequested(AttachRequest),
    ClientDisconnected,
    DeleteSessionRequested,
    PingRequested,
    UnexpectedRequest(ClientRequest),
}

impl SessionHandshakeMessage {
    pub fn from_first_request(request: Option<ClientRequest>) -> Self {
        match request {
            Some(ClientRequest::Attach(attach_request)) => Self::AttachRequested(attach_request),
            Some(ClientRequest::DeleteSession) => Self::DeleteSessionRequested,
            Some(ClientRequest::Ping) => Self::PingRequested,
            Some(request) => Self::UnexpectedRequest(request),
            None => Self::ClientDisconnected,
        }
    }
}

#[derive(Debug)]
pub enum SessionClientMessage {
    ClientDisconnected,
    Request(ClientRequest),
}

impl SessionClientMessage {
    pub fn from_request(request: Option<ClientRequest>) -> Self {
        request.map_or(Self::ClientDisconnected, Self::Request)
    }
}

pub struct SessionClientHandshake {
    pub connection: ServerConnection,
    pub message: SessionHandshakeMessage,
}

#[derive(Debug)]
pub enum SessionHandshakeOutcome {
    AttachAccepted(AttachRequest),
    DeleteSessionRequested,
    NoClient,
    Respond(ServerEvent),
}

pub enum SessionClientTaskMessage {
    Finished(SessionRuntimeState),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPaneOutputMessage {
    PaneExited,
    PaneOutputReady,
}

impl From<PtyEvent> for SessionPaneOutputMessage {
    fn from(event: PtyEvent) -> Self {
        match event {
            PtyEvent::Exited => Self::PaneExited,
            PtyEvent::OutputReady => Self::PaneOutputReady,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRuntimeTimerMessage {
    CmdHandoffSampleReady,
    HeartbeatTick,
    RenderDeadlineReached,
    TrackedProcessQuietDeadlineReached,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRuntimeShutdownMessage {
    AcceptedConnectionLimitReached,
    DeleteSessionRequested,
    FinalPaneExited,
    PaneRuntimeSetEmpty,
}

impl SessionRuntimeShutdownMessage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AcceptedConnectionLimitReached => "accepted_connection_limit_reached",
            Self::DeleteSessionRequested => "delete_session_requested",
            Self::FinalPaneExited => "final_pane_exited",
            Self::PaneRuntimeSetEmpty => "pane_runtime_set_empty",
        }
    }
}

pub struct SessionRuntimeState {
    // Client-session execution owns layout and runtimes while active; SessionRuntime keeps the handoff slot private.
    layout: SessionLayout,
    pane_runtimes: PaneRuntimes,
}

pub struct ClientSessionTaskRuntime {
    completion_sender: tokio::sync::mpsc::Sender<SessionClientTaskMessage>,
    delete_sessions: Arc<DeleteSessions>,
    state: SessionRuntimeState,
}

impl ClientSessionTaskRuntime {
    #[tracing::instrument(name = "muxr_session", skip_all, fields(session = %config.session))]
    pub async fn run_client_session(
        mut self,
        config: &ServerConfig,
        connection: ServerConnection,
        attach_request: AttachRequest,
    ) -> rootcause::Result<()> {
        let result = crate::client::session::handle_client(
            config,
            connection,
            attach_request,
            &self.delete_sessions,
            &mut self.state.layout,
            &mut self.state.pane_runtimes,
        )
        .await;
        match self
            .completion_sender
            .try_send(SessionClientTaskMessage::Finished(self.state))
        {
            // Closed means the session loop is already gone; a full channel can strand live state without another
            // recovery path.
            Ok(()) | Err(TrySendError::Closed(_)) => {}
            Err(TrySendError::Full(_)) => {
                crate::session::tracing::client::state_handoff_failed("channel_full");
            }
        }
        result
    }
}

pub struct SessionRuntime {
    state: Option<SessionRuntimeState>,
}

impl SessionRuntime {
    pub fn spawn(
        config: &ServerConfig,
        initial_size: &TerminalSize,
        pane_exit_notify: Arc<tokio::sync::Notify>,
    ) -> rootcause::Result<Self> {
        let metadata = SessionMetadata::try_from(config)?;
        let start_seed = SessionStartSeed::load(config, metadata)?;
        let pane_runtimes = PaneRuntimes::spawn_for_start_seed(config, &start_seed, initial_size, pane_exit_notify)?;
        Ok(Self {
            state: Some(SessionRuntimeState {
                layout: start_seed.layout,
                pane_runtimes,
            }),
        })
    }

    pub fn persist_metadata(&self, paths: &SessionPaths) -> rootcause::Result<()> {
        crate::state::persisted::write_metadata(paths, &self.state_ref()?.layout)
    }

    pub fn handle_handshake_message(
        &self,
        config: &ServerConfig,
        message: SessionHandshakeMessage,
    ) -> SessionHandshakeOutcome {
        match message {
            SessionHandshakeMessage::AttachRequested(attach_request) => {
                self.handle_attach_request(config, attach_request)
            }
            SessionHandshakeMessage::ClientDisconnected => SessionHandshakeOutcome::NoClient,
            SessionHandshakeMessage::DeleteSessionRequested => SessionHandshakeOutcome::DeleteSessionRequested,
            SessionHandshakeMessage::PingRequested => SessionHandshakeOutcome::Respond(ServerEvent::Pong),
            SessionHandshakeMessage::UnexpectedRequest(request) => {
                SessionHandshakeOutcome::Respond(ServerEvent::Error(ServerError::unexpected_request(request)))
            }
        }
    }

    pub fn handle_client_task_message(&mut self, message: SessionClientTaskMessage) {
        match message {
            SessionClientTaskMessage::Finished(state) => self.state = Some(state),
        }
    }

    pub fn client_session_task_runtime(
        &mut self,
        completion_sender: tokio::sync::mpsc::Sender<SessionClientTaskMessage>,
        delete_sessions: Arc<DeleteSessions>,
    ) -> rootcause::Result<ClientSessionTaskRuntime> {
        Ok(ClientSessionTaskRuntime {
            completion_sender,
            delete_sessions,
            state: self.take_state_for_attach()?,
        })
    }

    pub fn reap_exited_panes(&mut self, config: &ServerConfig) -> rootcause::Result<ReapResult> {
        let Some(state) = &mut self.state else {
            return Ok(ReapResult::NoExitedPanes);
        };
        self::reap_exited_panes(config, &mut state.layout, &mut state.pane_runtimes)
    }

    pub fn pane_runtime_set_status(&self) -> PaneRuntimeSetStatus {
        match &self.state {
            Some(state) if state.pane_runtimes.set_status() == PaneRuntimeSetStatus::Empty => {
                PaneRuntimeSetStatus::Empty
            }
            Some(_) | None => PaneRuntimeSetStatus::HasPanes,
        }
    }

    fn take_state_for_attach(&mut self) -> rootcause::Result<SessionRuntimeState> {
        self.state
            .take()
            .ok_or_else(|| rootcause::report!("muxr session runtime state is already attached"))
    }

    fn handle_attach_request(&self, config: &ServerConfig, attach_request: AttachRequest) -> SessionHandshakeOutcome {
        if self.state.is_none() {
            return SessionHandshakeOutcome::Respond(ServerEvent::Error(ServerError::ClientAlreadyAttached));
        }
        if attach_request.session != config.session {
            return SessionHandshakeOutcome::Respond(ServerEvent::Error(ServerError::SessionMismatch {
                expected: config.session.clone(),
                actual: attach_request.session,
            }));
        }
        SessionHandshakeOutcome::AttachAccepted(attach_request)
    }

    fn state_ref(&self) -> rootcause::Result<&SessionRuntimeState> {
        self.state
            .as_ref()
            .ok_or_else(|| rootcause::report!("muxr session runtime state is attached"))
    }
}

pub fn reap_exited_panes(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &mut PaneRuntimes,
) -> rootcause::Result<ReapResult> {
    let exited_panes = runtimes.exited_panes()?;
    if exited_panes.is_empty() {
        return Ok(ReapResult::NoExitedPanes);
    }

    let exited_at = crate::server::unix_timestamp_millis()?;
    let mut final_pane_removed = false;
    let _synced = runtimes.sync_layout_terminal_titles(layout);
    let mut removed_panes = Vec::new();
    for (pane_id, exit_status) in &exited_panes {
        match layout.remove_exited_pane(*pane_id, exited_at, exit_status.clone())? {
            PaneExitOutcome::Final => final_pane_removed = true,
            PaneExitOutcome::Removed => {}
        }
        removed_panes.push(*pane_id);
    }
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    for pane_id in &removed_panes {
        runtimes.remove(*pane_id);
    }

    if final_pane_removed {
        Ok(ReapResult::Final)
    } else {
        Ok(ReapResult::Removed {
            pane_ids: removed_panes,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use muxr_core::ClientRequest;
    use muxr_transport::ClientConnection;
    use muxr_transport::ServerListener;
    use rootcause::report;
    use test_that::prelude::*;

    use super::*;
    use crate::pane::runtime::test_helpers as pane_runtime_test_helpers;
    use crate::server::test_helpers as server_test_helpers;
    use crate::state::SessionMetadata;

    #[test]
    fn test_run_client_session_when_completion_channel_full_warns_with_session_span() -> rootcause::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let tempdir = tempfile::tempdir()?;
        let config = server_test_helpers::server_config(tempdir.path(), "work")?;
        let session = config.session.clone();

        let log = crate::session::tracing::collect_test_log(&session, || {
            runtime.block_on(async {
                let terminal_size = TerminalSize::new(80, 24)?;
                let attach_request = AttachRequest {
                    session: config.session.clone(),
                    terminal_size,
                };

                crate::session::files::prepare_session_dirs(&config.paths)?;
                let mut session_runtime = SessionRuntime::spawn(
                    &config,
                    &attach_request.terminal_size,
                    Arc::new(tokio::sync::Notify::new()),
                )?;
                let (completion_sender, _completion_receiver) = tokio::sync::mpsc::channel(1);
                let blocked_state = SessionRuntimeState {
                    layout: SessionLayout::initial(&config.session, self::metadata("sh", 1))?,
                    pane_runtimes: pane_runtime_test_helpers::empty_runtimes(),
                };
                completion_sender
                    .send(SessionClientTaskMessage::Finished(blocked_state))
                    .await
                    .map_err(|_| report!("failed to pre-fill client-session completion channel"))?;
                let task_runtime = session_runtime
                    .client_session_task_runtime(completion_sender, Arc::new(DeleteSessions::default()))?;
                let listener = ServerListener::bind(&config.paths.socket)?;
                let (mut client_connection, server_connection) =
                    tokio::try_join!(ClientConnection::connect(&config.paths.socket), listener.accept())?;

                let client_session = task_runtime.run_client_session(&config, server_connection, attach_request);
                let detached_client = async {
                    client_connection.send_request(&ClientRequest::Detach).await?;
                    self::read_connection_until_detached(&mut client_connection).await
                };
                let (client_session_result, detached_client_result) = tokio::join!(client_session, detached_client);
                client_session_result?;
                detached_client_result?;
                Ok(())
            })
        })?;

        assert_that!(log, contains_substring("kind=\"client_session_state_handoff_failed\""));
        assert_that!(log, contains_substring("reason=\"channel_full\""));
        assert_that!(log, contains_substring("session=work"));
        Ok(())
    }

    async fn read_connection_until_detached(connection: &mut ClientConnection) -> rootcause::Result<()> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(report!("timed out waiting for muxr detach ack"));
            }

            match tokio::time::timeout(Duration::from_millis(50), connection.recv_event()).await {
                Ok(Ok(Some(ServerEvent::Detached))) => return Ok(()),
                Ok(Ok(Some(ServerEvent::Ping))) => connection.send_request(&ClientRequest::Pong).await?,
                Ok(Ok(Some(ServerEvent::Error(error)))) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                Ok(Ok(Some(
                    ServerEvent::Attached(_)
                    | ServerEvent::Deleted
                    | ServerEvent::Pong
                    | ServerEvent::Layout(_)
                    | ServerEvent::SidebarLayout(_)
                    | ServerEvent::PaneRegions(_)
                    | ServerEvent::Render(_)
                    | ServerEvent::ScrollPaneLineResult { .. },
                )))
                | Err(_) => {}
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) => return Err(report!("expected detached event")),
            }
        }
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }
}
