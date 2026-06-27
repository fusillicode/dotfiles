use std::fs;

use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TabId;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

use crate::state::SessionLayout;
use crate::state::Tab;
use crate::state::VERSION;

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

impl SessionLayout {
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
            entries: persisted.tabs,
            session: persisted.session,
        };
        // Persisted layout bypasses constructors; rebuilding a snapshot validates tab and pane invariants.
        layout.snapshot()?;
        Ok(layout)
    }
}

pub fn write_metadata(paths: &SessionPaths, layout: &SessionLayout) -> rootcause::Result<()> {
    let layout = PersistedLayout {
        version: VERSION,
        session: &layout.session,
        active_tab: &layout.active_tab,
        tabs: &layout.entries,
    };
    let encoded = serde_json::to_vec_pretty(&layout).context("failed to encode muxr layout metadata")?;
    fs::write(&paths.layout, encoded).context("failed to write muxr layout metadata")?;
    Ok(())
}

pub fn load_metadata(paths: &SessionPaths, session: &SessionName) -> rootcause::Result<Option<SessionLayout>> {
    // Read is the source of truth: a separate exists check can race with cleanup,
    // while NotFound still means there is no persisted layout to restore.
    let bytes = match fs::read(&paths.layout) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("failed to read muxr layout metadata")?,
    };
    let persisted: PersistedLayoutOwned =
        serde_json::from_slice(&bytes).context("failed to parse muxr layout metadata")?;

    Ok(Some(SessionLayout::from_persisted(session, persisted)?))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use muxr_config::MuxrConfig;
    use muxr_core::TerminalSize;

    use super::*;
    use crate::pane::resize::PaneResizeDirection;
    use crate::pane::split::PaneSplitAxis;
    use crate::state::test_helpers as state_test_helpers;

    #[test]
    fn test_layout_metadata_when_nested_panes_exist_round_trips_tree() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        let mut layout = SessionLayout::initial(&session, state_test_helpers::metadata("sh", 1))?;

        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 3),
            PaneSplitAxis::Horizontal,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;
        self::write_metadata(&paths, &layout)?;

        let loaded =
            self::load_metadata(&paths, &session)?.ok_or_else(|| report!("expected muxr layout metadata to load"))?;

        pretty_assertions::assert_eq!(loaded.active_pane_id()?.to_string(), "pane-3");
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_regions(&loaded, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 12),
                ("pane-3".to_owned(), 41, 13, 39, 11),
            ],
        );
        Ok(())
    }

    #[test]
    fn test_layout_metadata_when_resized_panes_exist_round_trips_split_ratio() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        fs::create_dir_all(&paths.root)?;
        let mut layout = SessionLayout::initial(&session, state_test_helpers::metadata("sh", 1))?;

        layout.split_active_pane(
            MuxrConfig::default().layout,
            state_test_helpers::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;
        pretty_assertions::assert_eq!(
            layout.resize_active_pane(MuxrConfig::default().layout, PaneResizeDirection::Left)?,
            crate::pane::resize::PaneResizeChange::Changed,
        );
        self::write_metadata(&paths, &layout)?;

        let loaded =
            self::load_metadata(&paths, &session)?.ok_or_else(|| report!("expected muxr layout metadata to load"))?;

        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_regions(&loaded, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 36, 24),
                ("pane-2".to_owned(), 37, 0, 43, 24),
            ],
        );
        Ok(())
    }

    fn session_paths(base: &std::path::Path, raw: &str) -> rootcause::Result<(SessionName, SessionPaths)> {
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
}
