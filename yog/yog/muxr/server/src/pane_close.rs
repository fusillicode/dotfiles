use std::sync::Mutex;

use muxr_core::PaneId;
use rootcause::report;

use crate::pane_runtime::PaneRuntimes;
use crate::pty::PtyExitStatus;
use crate::server::ServerConfig;
use crate::state::PaneTree;
use crate::state::SessionLayout;
use crate::state::Tab;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClosePaneOutcome {
    Final { pane_id: PaneId },
    Removed { pane_id: PaneId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneExitOutcome {
    Final,
    Removed,
}

impl SessionLayout {
    pub fn close_active_pane(&mut self, exited_at: u64) -> rootcause::Result<ClosePaneOutcome> {
        let active_tab_index = self.active_tab_index()?;
        let final_pane = self.entries.len() == 1
            && self
                .entries
                .get(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
                .pane_count()
                == 1;
        let active_pane = self
            .entries
            .get(active_tab_index)
            .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
            .active_pane;

        if final_pane {
            self.entries
                .get_mut(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
                .mark_pane_closed(active_pane, exited_at)?;
            return Ok(ClosePaneOutcome::Final { pane_id: active_pane });
        }

        if self
            .entries
            .get(active_tab_index)
            .ok_or_else(|| report!("muxr active tab index is outside server layout"))?
            .pane_count()
            == 1
        {
            self.remove_tab_at(active_tab_index)?;
        } else {
            let tab = self
                .entries
                .get_mut(active_tab_index)
                .ok_or_else(|| report!("muxr active tab index is outside server layout"))?;
            let fallback_pane = tab.remove_pane(active_pane)?;
            let _focused = tab.focus_pane(fallback_pane)?;
        }

        Ok(ClosePaneOutcome::Removed { pane_id: active_pane })
    }

    pub fn remove_exited_pane(
        &mut self,
        pane_id: PaneId,
        exited_at: u64,
        exit_status: PtyExitStatus,
    ) -> rootcause::Result<PaneExitOutcome> {
        let tab_index = self.pane_tab_index(pane_id)?;

        if self.entries.len() == 1
            && self
                .entries
                .get(tab_index)
                .ok_or_else(|| report!("muxr exited pane tab is missing"))?
                .pane_count()
                == 1
        {
            let tab = self
                .entries
                .get_mut(tab_index)
                .ok_or_else(|| report!("muxr final pane tab is missing"))?;
            tab.mark_pane_process_exited(pane_id, exited_at, exit_status)?;
            return Ok(PaneExitOutcome::Final);
        }

        if self
            .entries
            .get(tab_index)
            .ok_or_else(|| report!("muxr exited pane tab is missing"))?
            .pane_count()
            == 1
        {
            self.remove_tab_at(tab_index)?;
            return Ok(PaneExitOutcome::Removed);
        }

        let tab = self
            .entries
            .get_mut(tab_index)
            .ok_or_else(|| report!("muxr exited pane tab is missing"))?;
        let removed_active_pane = tab.active_pane == pane_id;
        let fallback_pane = tab.remove_pane(pane_id)?;
        if removed_active_pane {
            let _focused = tab.focus_pane(fallback_pane)?;
        }
        Ok(PaneExitOutcome::Removed)
    }
}

impl Tab {
    pub fn remove_pane(&mut self, pane_id: PaneId) -> rootcause::Result<PaneId> {
        self.pane_tree.remove_pane(pane_id)
    }

    fn mark_pane_closed(&mut self, pane_id: PaneId, exited_at: u64) -> rootcause::Result<()> {
        let Some(pane) = self.pane_tree.pane_mut(pane_id) else {
            return Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")));
        };

        pane.mark_closed(exited_at);
        Ok(())
    }

    fn mark_pane_process_exited(
        &mut self,
        pane_id: PaneId,
        exited_at: u64,
        exit_status: PtyExitStatus,
    ) -> rootcause::Result<()> {
        let Some(pane) = self.pane_tree.pane_mut(pane_id) else {
            return Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")));
        };

        pane.mark_process_exited(exited_at, exit_status);
        Ok(())
    }
}

impl PaneTree {
    pub fn remove_pane(&mut self, pane_id: PaneId) -> rootcause::Result<PaneId> {
        let Some(fallback_pane) = self.remove_pane_from_split(pane_id)? else {
            return Err(report!("muxr pane is missing from server layout").attach(format!("pane_id={pane_id}")));
        };
        Ok(fallback_pane)
    }

    fn remove_pane_from_split(&mut self, pane_id: PaneId) -> rootcause::Result<Option<PaneId>> {
        match self {
            Self::Pane(pane) if pane.id == pane_id => {
                Err(report!("muxr cannot remove a pane without a sibling").attach(format!("pane_id={pane_id}")))
            }
            Self::Split { first, second, .. } if first.contains_pane(pane_id) => {
                if first.pane_count() == 1 {
                    let replacement = (**second).clone();
                    let fallback_pane = replacement.first_pane_id();
                    *self = replacement;
                    Ok(Some(fallback_pane))
                } else {
                    first.remove_pane_from_split(pane_id)
                }
            }
            Self::Split { first, second, .. } if second.contains_pane(pane_id) => {
                if second.pane_count() == 1 {
                    let replacement = (**first).clone();
                    let fallback_pane = replacement.first_pane_id();
                    *self = replacement;
                    Ok(Some(fallback_pane))
                } else {
                    second.remove_pane_from_split(pane_id)
                }
            }
            Self::Pane(_) | Self::Split { .. } => Ok(None),
        }
    }

    fn first_pane_id(&self) -> PaneId {
        match self {
            Self::Pane(pane) => pane.id,
            Self::Split { first, .. } => first.first_pane_id(),
        }
    }
}

pub fn handle_close_pane_cmd(
    config: &ServerConfig,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<ClosePaneOutcome> {
    let exited_at = crate::server::unix_timestamp_millis()?;
    let mut layout = crate::server::lock_mutex(layout, "layout")?;
    // Closing removes the runtime, so any title-derived cwd must be synced before queued PTY events disappear.
    crate::pane_runtime::sync_layout_terminal_titles(&mut layout, runtimes)?;
    let outcome = layout.close_active_pane(exited_at)?;
    let pane_id = match &outcome {
        ClosePaneOutcome::Final { pane_id } | ClosePaneOutcome::Removed { pane_id } => *pane_id,
    };
    {
        let mut runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
        runtimes.remove(pane_id);
        drop(runtimes);
    }
    crate::state::persisted::write_metadata(&config.paths, &layout)?;
    drop(layout);
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use muxr_core::TerminalSize;

    use super::*;
    use crate::pane_split::PaneSplitAxis;
    use crate::state::test_helpers as state_test_helpers;

    #[test]
    fn test_layout_split_and_close_when_multiple_panes_updates_active_pane() -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;

        let pane_id = layout.split_active_pane(state_test_helpers::metadata("sh", 2), PaneSplitAxis::Vertical)?;

        pretty_assertions::assert_eq!(pane_id.to_string(), "pane-2");
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_ids(&layout)?,
            vec!["pane-1", "pane-2"]
        );

        let close = layout.close_active_pane(3)?;

        pretty_assertions::assert_eq!(
            close,
            ClosePaneOutcome::Removed {
                pane_id: PaneId::new(2)?,
            },
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-1");
        pretty_assertions::assert_eq!(state_test_helpers::layout_active_tab_pane_ids(&layout)?, vec!["pane-1"]);
        Ok(())
    }

    #[test]
    fn test_layout_close_when_nested_pane_closes_collapses_parent_split() -> rootcause::Result<()> {
        let mut layout = state_test_helpers::layout("work")?;

        layout.split_active_pane(state_test_helpers::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(state_test_helpers::metadata("sh", 3), PaneSplitAxis::Horizontal)?;
        state_test_helpers::force_balanced_test_split_ratio(&mut layout)?;
        let close = layout.close_active_pane(3)?;

        pretty_assertions::assert_eq!(
            close,
            ClosePaneOutcome::Removed {
                pane_id: PaneId::new(3)?,
            },
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_ids(&layout)?,
            vec!["pane-1", "pane-2"]
        );
        pretty_assertions::assert_eq!(
            state_test_helpers::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 24),
            ],
        );
        Ok(())
    }
}
