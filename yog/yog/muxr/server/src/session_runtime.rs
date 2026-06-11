pub use attached_session::AttachedClientTaskRuntime;
pub use attached_session::ReapResult;
#[cfg(test)]
pub use attached_session::initial_attached_render;
#[cfg(test)]
pub use attached_session::resize_panes_to_layout;
use muxr_core::AttachRequest;
use muxr_core::ClientRequest;
use muxr_core::ServerError;
use muxr_core::ServerEvent;
use muxr_core::SessionPaths;
use muxr_core::TerminalSize;
use muxr_transport::ServerConnection;

use crate::pane_runtime::PaneRuntimes;
use crate::pty::PtyEvent;
use crate::server::ServerConfig;
use crate::session_start_seed::SessionStartSeed;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;

mod attached_session;

/// High-volume PTY output is bounded and coalesced: a full queue keeps one pending output-ready marker instead of
/// enqueueing every frame. Session runtime routing must preserve that behavior while keeping PTY state single-owned.
pub const PANE_OUTPUT_EVENT_CHANNEL_LIMIT: usize = 1024;
pub const CLIENT_HANDSHAKE_CHANNEL_LIMIT: usize = 32;

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
pub enum SessionAttachedClientMessage {
    ClientDisconnected,
    Request(ClientRequest),
}

impl SessionAttachedClientMessage {
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

pub enum SessionAttachedClientTaskMessage {
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
    RenderTick,
    ShellPollTick,
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
    layout: SessionLayout,
    pane_runtimes: PaneRuntimes,
}

pub struct SessionRuntime {
    state: Option<SessionRuntimeState>,
}

impl SessionRuntime {
    pub fn spawn(config: &ServerConfig, initial_size: &TerminalSize) -> rootcause::Result<Self> {
        let metadata = SessionMetadata::try_from(config)?;
        let start_seed = SessionStartSeed::load(config, metadata)?;
        let pane_runtimes = PaneRuntimes::spawn_for_start_seed(config, &start_seed, initial_size)?;
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

    pub fn handle_attached_client_task_message(&mut self, message: SessionAttachedClientTaskMessage) {
        match message {
            SessionAttachedClientTaskMessage::Finished(state) => self.state = Some(state),
        }
    }

    pub fn take_state_for_attach(&mut self) -> rootcause::Result<SessionRuntimeState> {
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
