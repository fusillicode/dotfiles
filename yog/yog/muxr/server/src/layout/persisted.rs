use std::fs;

use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TabId;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use crate::layout::Layout;
use crate::layout::Tab;
use crate::layout::VERSION;

#[derive(Serialize)]
struct PersistedLayout<'a> {
    version: u16,
    session: &'a SessionName,
    active_tab: &'a TabId,
    tabs: &'a [Tab],
}

#[derive(Deserialize)]
struct PersistedLayoutOwned {
    version: u16,
    session: SessionName,
    active_tab: TabId,
    tabs: Vec<Tab>,
}

impl Layout {
    fn from_persisted(session: &SessionName, persisted: PersistedLayoutOwned) -> rootcause::Result<Self> {
        if persisted.version != VERSION {
            return Err(report!("unsupported muxr layout metadata version")
                .attach(format!("expected={VERSION}"))
                .attach(format!("actual={}", persisted.version)));
        }
        if persisted.session != *session {
            return Err(report!("muxr layout metadata session mismatch")
                .attach(format!("expected={session}"))
                .attach(format!("actual={}", persisted.session)));
        }

        let layout = Self {
            active_tab: persisted.active_tab,
            session: persisted.session,
            tabs: persisted.tabs,
        };
        // Persisted layout bypasses constructors; rebuilding a snapshot validates tab and pane invariants.
        layout.snapshot()?;
        Ok(layout)
    }
}

pub fn write_metadata(paths: &SessionPaths, layout: &Layout) -> rootcause::Result<()> {
    let layout = PersistedLayout {
        version: VERSION,
        session: &layout.session,
        active_tab: &layout.active_tab,
        tabs: &layout.tabs,
    };
    let encoded = serde_json::to_vec_pretty(&layout).context("failed to encode muxr layout metadata")?;
    fs::write(&paths.layout, encoded).context("failed to write muxr layout metadata")?;
    Ok(())
}

pub fn load_metadata(paths: &SessionPaths, session: &SessionName) -> rootcause::Result<Option<Layout>> {
    // Read is the source of truth: a separate exists check can race with cleanup,
    // while NotFound still means there is no persisted layout to restore.
    let bytes = match fs::read(&paths.layout) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("failed to read muxr layout metadata")?,
    };
    let persisted: PersistedLayoutOwned =
        serde_json::from_slice(&bytes).context("failed to parse muxr layout metadata")?;

    Ok(Some(Layout::from_persisted(session, persisted)?))
}
