use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use muxr_core::ServerEvent;
use muxr_core::SessionPaths;
use muxr_transport::ServerConnection;
use muxr_transport::ServerEventWriter;
use rootcause::prelude::ResultExt;

#[derive(Debug, Default)]
pub struct DeleteSessions {
    requested: AtomicBool,
}

impl DeleteSessions {
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }
}

pub async fn handle_handshake_delete(
    connection: &mut ServerConnection,
    delete_sessions: &DeleteSessions,
    client_write_timeout: Duration,
) -> rootcause::Result<()> {
    delete_sessions.request();
    let _sent =
        self::send_connection_event_with_timeout(connection, &ServerEvent::Deleted, client_write_timeout).await?;
    Ok(())
}

pub async fn handle_attached_delete(
    event_writer: &mut ServerEventWriter,
    delete_sessions: &DeleteSessions,
    client_write_timeout: Duration,
) -> rootcause::Result<()> {
    delete_sessions.request();
    let _sent = self::send_writer_event_with_timeout(event_writer, &ServerEvent::Deleted, client_write_timeout).await?;
    Ok(())
}

pub fn remove_session_files(paths: &SessionPaths) -> rootcause::Result<()> {
    // Live deletion is server-owned so pane processes and history writers are dropped before state removal.
    match fs::remove_dir_all(&paths.root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to remove muxr session dir")?,
    }
    match fs::remove_file(&paths.socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to remove muxr session socket")?,
    }
    Ok(())
}

async fn send_connection_event_with_timeout(
    connection: &mut ServerConnection,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> rootcause::Result<bool> {
    match tokio::time::timeout(client_write_timeout, connection.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}

async fn send_writer_event_with_timeout(
    writer: &mut ServerEventWriter,
    event: &ServerEvent,
    client_write_timeout: Duration,
) -> rootcause::Result<bool> {
    match tokio::time::timeout(client_write_timeout, writer.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}
