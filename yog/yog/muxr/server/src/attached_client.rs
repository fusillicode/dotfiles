use std::sync::Arc;
use std::time::Duration;

use muxr_core::AttachRequest;
use muxr_core::ServerEvent;
use muxr_transport::ServerConnection;
use rootcause::report;

use crate::server::ServerConfig;
use crate::session_runtime::AttachedClientTaskRuntime;
use crate::session_runtime::SessionClientHandshake;
use crate::session_runtime::SessionHandshakeMessage;

const CLIENT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

// Client task completion must wake the server even if the task returns an error or panics; otherwise task joins would
// rely on the old lifecycle poll.
struct TaskFinishedNotify(Arc<tokio::sync::Notify>);

impl Drop for TaskFinishedNotify {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

pub fn spawn_client_handshake_task(
    connection: ServerConnection,
    handshake_sender: &tokio::sync::mpsc::Sender<SessionClientHandshake>,
    task_finished_notify: Arc<tokio::sync::Notify>,
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) {
    let handshake_sender = handshake_sender.clone();
    handles.push(tokio::spawn(async move {
        let _task_finished = TaskFinishedNotify(task_finished_notify);
        let mut connection = connection;
        let message = self::read_client_handshake(&mut connection).await?;
        if handshake_sender
            .send(SessionClientHandshake { connection, message })
            .await
            .is_err()
        {
            return Ok(());
        }
        Ok(())
    }));
}

pub fn spawn_attached_client_task(
    config: &ServerConfig,
    runtime: AttachedClientTaskRuntime,
    connection: ServerConnection,
    attach_request: AttachRequest,
    task_finished_notify: Arc<tokio::sync::Notify>,
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) {
    let config = config.clone();
    handles.push(tokio::spawn(async move {
        let _task_finished = TaskFinishedNotify(task_finished_notify);
        runtime.run_attached_client(&config, connection, attach_request).await
    }));
}

pub async fn join_client_tasks(handles: Vec<tokio::task::JoinHandle<rootcause::Result<()>>>) -> rootcause::Result<()> {
    for handle in handles {
        self::join_client_task(handle).await?;
    }
    Ok(())
}

pub async fn join_finished_client_tasks(
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) -> rootcause::Result<()> {
    let mut pending_handles = Vec::new();
    for handle in handles.drain(..) {
        if handle.is_finished() {
            self::join_client_task(handle).await?;
        } else {
            pending_handles.push(handle);
        }
    }
    *handles = pending_handles;
    Ok(())
}

pub async fn read_client_handshake(connection: &mut ServerConnection) -> rootcause::Result<SessionHandshakeMessage> {
    match tokio::time::timeout(CLIENT_HANDSHAKE_TIMEOUT, connection.recv_request()).await {
        Ok(Ok(request)) => Ok(SessionHandshakeMessage::from_first_request(request)),
        Ok(Err(error)) => Err(error),
        Err(_elapsed) => Ok(SessionHandshakeMessage::ClientDisconnected),
    }
}

/// Send one event on a pre-attach connection with the server's bounded write timeout.
///
/// # Errors
/// This function currently returns `Ok(false)` for send failures and timeouts instead of an error.
pub async fn send_connection_event_with_timeout(
    connection: &mut ServerConnection,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> rootcause::Result<bool> {
    match tokio::time::timeout(client_write_timeout, connection.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}

async fn join_client_task(handle: tokio::task::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
    handle
        .await
        .unwrap_or_else(|error| Err(report!("muxr server client task panicked").attach(format!("{error}"))))
}
