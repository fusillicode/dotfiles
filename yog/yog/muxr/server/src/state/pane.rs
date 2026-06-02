use muxr_core::PaneId;
use muxr_core::PaneSnapshot;
use serde::Deserialize;
use serde::Serialize;

use crate::pane_split::PaneSplitAxis;
use crate::pane_split::PaneSplitRatio;
use crate::pty::PtyExitStatus;

// Pane splits are a tree so a new split mutates only the active pane subtree; a tab-wide axis would reflow siblings.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneTree {
    Pane(Pane),
    Split {
        axis: PaneSplitAxis,
        first_ratio: PaneSplitRatio,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl PaneTree {
    pub fn pane_count(&self) -> usize {
        match self {
            Self::Pane(_) => 1,
            Self::Split { first, second, .. } => first.pane_count().saturating_add(second.pane_count()),
        }
    }

    pub fn contains_pane(&self, pane_id: &PaneId) -> bool {
        match self {
            Self::Pane(pane) => pane.id == *pane_id,
            Self::Split { first, second, .. } => first.contains_pane(pane_id) || second.contains_pane(pane_id),
        }
    }

    pub fn pane_mut(&mut self, pane_id: &PaneId) -> Option<&mut Pane> {
        match self {
            Self::Pane(pane) if pane.id == *pane_id => Some(pane),
            Self::Pane(_) => None,
            Self::Split { first, second, .. } => first.pane_mut(pane_id).or_else(|| second.pane_mut(pane_id)),
        }
    }

    pub fn append_pane_ids<'a>(&'a self, ids: &mut Vec<&'a str>) {
        match self {
            Self::Pane(pane) => ids.push(pane.id.as_ref()),
            Self::Split { first, second, .. } => {
                first.append_pane_ids(ids);
                second.append_pane_ids(ids);
            }
        }
    }

    pub fn append_panes<'a>(&'a self, panes: &mut Vec<&'a Pane>) {
        match self {
            Self::Pane(pane) => panes.push(pane),
            Self::Split { first, second, .. } => {
                first.append_panes(panes);
                second.append_panes(panes);
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Pane {
    pub cmd_label: String,
    pub cwd: String,
    pub focus_seq: u64,
    pub id: PaneId,
    pub started_at: u64,
    pub state: PaneState,
    pub title: String,
}

impl Pane {
    pub const fn set_focus_seq(&mut self, focus_seq: u64) {
        self.focus_seq = focus_seq;
    }

    pub fn mark_closed(&mut self, at: u64) {
        self.state = PaneState::Closed { at };
    }

    pub fn mark_process_exited(&mut self, at: u64, status: PtyExitStatus) {
        self.state = PaneState::ProcessExited { at, status };
    }

    /// Refresh pane cwd metadata from path-like shell title updates.
    pub fn sync_terminal_title(&mut self, terminal_title: Option<&str>) -> bool {
        if let Some(cwd) = crate::cmd_label::classify_terminal_title(terminal_title, &self.cwd).cwd {
            if self.cwd == cwd {
                return false;
            }
            self.cwd = cwd;
            return true;
        }
        false
    }

    /// Build a client snapshot with tab bar cwd/cmd metadata derived from the latest terminal title.
    pub fn snapshot_with_terminal_title(&self, terminal_title: Option<&str>) -> PaneSnapshot {
        let terminal_title = crate::cmd_label::classify_terminal_title(terminal_title, &self.cwd);
        PaneSnapshot {
            cmd_label: terminal_title.cmd_label,
            cwd: terminal_title.cwd.unwrap_or_else(|| self.cwd.clone()),
            id: self.id.clone(),
            title: self.title.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneState {
    Running,
    Closed { at: u64 },
    ProcessExited { at: u64, status: PtyExitStatus },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_with_terminal_title_when_title_is_cwd_updates_snapshot_cwd() -> rootcause::Result<()> {
        let snapshot = self::pane()?.snapshot_with_terminal_title(Some("~"));

        pretty_assertions::assert_eq!(snapshot.cwd, "~");
        pretty_assertions::assert_eq!(snapshot.cmd_label, None);
        Ok(())
    }

    #[test]
    fn test_snapshot_with_terminal_title_when_title_is_cmd_keeps_pane_cwd() -> rootcause::Result<()> {
        let snapshot = self::pane()?.snapshot_with_terminal_title(Some("cargo test"));

        pretty_assertions::assert_eq!(snapshot.cwd, "/old/project");
        pretty_assertions::assert_eq!(snapshot.cmd_label, Some("cargo test".to_owned()));
        Ok(())
    }

    #[test]
    fn test_sync_terminal_title_when_title_is_cwd_updates_pane_cwd() -> rootcause::Result<()> {
        let mut pane = self::pane()?;

        assert2::assert!(pane.sync_terminal_title(Some("~")));

        pretty_assertions::assert_eq!(pane.cwd, "~");
        Ok(())
    }

    #[test]
    fn test_sync_terminal_title_when_title_is_same_cwd_returns_false() -> rootcause::Result<()> {
        let mut pane = self::pane()?;

        assert2::assert!(!pane.sync_terminal_title(Some("/old/project")));

        pretty_assertions::assert_eq!(pane.cwd, "/old/project");
        Ok(())
    }

    #[test]
    fn test_sync_terminal_title_when_title_is_cmd_returns_false() -> rootcause::Result<()> {
        let mut pane = self::pane()?;

        assert2::assert!(!pane.sync_terminal_title(Some("cargo test")));

        pretty_assertions::assert_eq!(pane.cwd, "/old/project");
        Ok(())
    }

    fn pane() -> rootcause::Result<Pane> {
        Ok(Pane {
            cmd_label: "zsh".to_owned(),
            cwd: "/old/project".to_owned(),
            focus_seq: 1,
            id: PaneId::new("pane-1")?,
            started_at: 1,
            state: PaneState::Running,
            title: "zsh".to_owned(),
        })
    }
}
