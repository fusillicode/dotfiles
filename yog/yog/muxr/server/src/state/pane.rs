use muxr_core::PaneId;
use muxr_core::PaneSnapshot;
use muxr_core::TrackedProcessState;
use serde::Deserialize;
use serde::Serialize;

use crate::cmd_label::TerminalTitle;
use crate::pane::split::PaneSplitAxis;
use crate::pane::split::PaneSplitRatio;
use crate::pty::PtyExitStatus;
use crate::state::session::PaneMetadataSync;

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

    pub fn contains_pane(&self, pane_id: PaneId) -> bool {
        match self {
            Self::Pane(pane) => pane.id == pane_id,
            Self::Split { first, second, .. } => first.contains_pane(pane_id) || second.contains_pane(pane_id),
        }
    }

    pub fn pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        match self {
            Self::Pane(pane) if pane.id == pane_id => Some(pane),
            Self::Pane(_) => None,
            Self::Split { first, second, .. } => first.pane_mut(pane_id).or_else(|| second.pane_mut(pane_id)),
        }
    }

    pub fn append_pane_ids(&self, ids: &mut Vec<PaneId>) {
        match self {
            Self::Pane(pane) => ids.push(pane.id),
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

    pub(crate) fn last_focused_pane(&self) -> &Pane {
        match self {
            Self::Pane(pane) => pane,
            Self::Split { first, second, .. } => {
                let first_pane = first.last_focused_pane();
                let second_pane = second.last_focused_pane();
                if first_pane.focus_seq >= second_pane.focus_seq {
                    first_pane
                } else {
                    second_pane
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum PaneAttentionState {
    #[default]
    Idle,
    NeedsAttention,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Pane {
    #[serde(default, skip_serializing)]
    pub attention_state: PaneAttentionState,
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
    pub fn sync_terminal_title(&mut self, terminal_title: Option<&str>) -> PaneMetadataSync {
        if let Some(cwd) = TerminalTitle::classify(terminal_title, &self.cwd).cwd {
            if self.cwd == cwd {
                return PaneMetadataSync::Unchanged;
            }
            self.cwd = cwd;
            return PaneMetadataSync::Changed;
        }
        PaneMetadataSync::Unchanged
    }

    /// Build a client snapshot with live runtime cmd metadata overriding decorative terminal titles.
    pub fn snapshot_with_runtime_metadata(
        &self,
        terminal_title: Option<&str>,
        runtime_cmd_label: Option<&str>,
        runtime_tracked_process_state: TrackedProcessState,
    ) -> PaneSnapshot {
        let terminal_title = TerminalTitle::classify(terminal_title, &self.cwd);
        PaneSnapshot {
            tracked_process_state: runtime_tracked_process_state,
            cmd_label: runtime_cmd_label
                .map(str::trim)
                .filter(|cmd| !cmd.is_empty())
                .map(ToOwned::to_owned)
                .or(terminal_title.cmd_label),
            cwd: terminal_title.cwd.unwrap_or_else(|| self.cwd.clone()),
            focus_seq: self.focus_seq,
            id: self.id,
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
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_snapshot_with_runtime_metadata_when_title_is_cwd_updates_snapshot_cwd() -> rootcause::Result<()> {
        let snapshot = self::pane()?.snapshot_with_runtime_metadata(Some("~"), None, TrackedProcessState::None);

        assert_that!(snapshot.cwd, eq("~"));
        assert_that!(snapshot.cmd_label, eq(None));
        Ok(())
    }

    #[test]
    fn test_snapshot_with_runtime_metadata_when_title_is_cmd_keeps_pane_cwd() -> rootcause::Result<()> {
        let snapshot =
            self::pane()?.snapshot_with_runtime_metadata(Some("cargo test"), None, TrackedProcessState::None);

        assert_that!(snapshot.cwd, eq("/old/project"));
        assert_that!(snapshot.cmd_label, eq(Some("cargo test".to_owned())));
        Ok(())
    }

    #[rstest::rstest]
    #[case::no_agent(TrackedProcessState::None)]
    #[case::seen(TrackedProcessState::Seen)]
    #[case::busy(TrackedProcessState::Busy)]
    #[case::unseen(TrackedProcessState::Unseen)]
    fn test_snapshot_with_runtime_metadata_when_runtime_tracked_process_state_varies_projects_state(
        #[case] tracked_process_state: TrackedProcessState,
    ) -> rootcause::Result<()> {
        let snapshot = self::pane()?.snapshot_with_runtime_metadata(Some("dotfiles"), None, tracked_process_state);

        assert_that!(snapshot.tracked_process_state, eq(tracked_process_state));
        assert_that!(snapshot.cmd_label, eq(Some("dotfiles".to_owned())));
        Ok(())
    }

    #[test]
    fn test_snapshot_with_runtime_metadata_when_runtime_cmd_is_present_overrides_terminal_title()
    -> rootcause::Result<()> {
        let snapshot =
            self::pane()?.snapshot_with_runtime_metadata(Some("dotfiles"), Some("codex"), TrackedProcessState::None);

        assert_that!(snapshot.cmd_label, eq(Some("codex".to_owned())));
        Ok(())
    }

    #[test]
    fn test_sync_terminal_title_when_title_is_cwd_updates_pane_cwd() -> rootcause::Result<()> {
        let mut pane = self::pane()?;

        assert_that!(pane.sync_terminal_title(Some("~")), eq(PaneMetadataSync::Changed));

        assert_that!(pane.cwd, eq("~"));
        Ok(())
    }

    #[test]
    fn test_sync_terminal_title_when_title_is_same_cwd_returns_false() -> rootcause::Result<()> {
        let mut pane = self::pane()?;

        assert_that!(
            pane.sync_terminal_title(Some("/old/project")),
            eq(PaneMetadataSync::Unchanged)
        );

        assert_that!(pane.cwd, eq("/old/project"));
        Ok(())
    }

    #[test]
    fn test_sync_terminal_title_when_title_is_cmd_returns_false() -> rootcause::Result<()> {
        let mut pane = self::pane()?;

        assert_that!(
            pane.sync_terminal_title(Some("cargo test")),
            eq(PaneMetadataSync::Unchanged)
        );

        assert_that!(pane.cwd, eq("/old/project"));
        Ok(())
    }

    fn pane() -> rootcause::Result<Pane> {
        Ok(Pane {
            attention_state: PaneAttentionState::Idle,
            cmd_label: "zsh".to_owned(),
            cwd: "/old/project".to_owned(),
            focus_seq: 1,
            id: PaneId::new(1)?,
            started_at: 1,
            state: PaneState::Running,
            title: "zsh".to_owned(),
        })
    }
}
