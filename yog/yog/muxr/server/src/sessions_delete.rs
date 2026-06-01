use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

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
) -> rootcause::Result<()> {
    delete_sessions.request();
    let _sent = crate::server::send_connection_event_with_timeout(connection, &ServerEvent::Deleted).await?;
    Ok(())
}

pub async fn handle_attached_delete(
    event_writer: &mut ServerEventWriter,
    delete_sessions: &DeleteSessions,
) -> rootcause::Result<()> {
    delete_sessions.request();
    let _sent = crate::server::send_writer_event_with_timeout(event_writer, &ServerEvent::Deleted).await?;
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
