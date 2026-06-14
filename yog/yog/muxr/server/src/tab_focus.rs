use std::time::Instant;

use muxr_core::PaneId;
use muxr_core::TabId;
use rootcause::report;

use crate::client_session::ClientSessionState;
use crate::pane_runtime::PaneRuntimes;
use crate::pane_tracked_process::PaneTrackedProcesses;
use crate::server::ServerConfig;
use crate::state::SessionLayout;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabFocusClientOutcome {
    Render { previous_pane: PaneId },
    Unchanged,
}

impl SessionLayout {
    pub fn focus_tab(&mut self, tab_id: TabId) -> rootcause::Result<bool> {
        if self.active_tab == tab_id {
            return Ok(false);
        }
        if !self.entries.iter().any(|tab| tab.id == tab_id) {
            return Ok(false);
        }
        self.active_tab = tab_id;
        let _acknowledged = self.acknowledge_active_pane_attention()?;
        Ok(true)
    }

    pub fn focus_previous_tab(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let previous_index = if tab_index == 0 {
            self.entries.len().saturating_sub(1)
        } else {
            tab_index.saturating_sub(1)
        };
        self.active_tab = self
            .entries
            .get(previous_index)
            .ok_or_else(|| report!("muxr previous tab is missing from server layout"))?
            .id;
        let _acknowledged = self.acknowledge_active_pane_attention()?;
        Ok(())
    }

    pub fn focus_next_tab(&mut self) -> rootcause::Result<()> {
        let tab_index = self.active_tab_index()?;
        let next_index = tab_index
            .checked_add(1)
            .filter(|index| *index < self.entries.len())
            .unwrap_or(0);
        self.active_tab = self
            .entries
            .get(next_index)
            .ok_or_else(|| report!("muxr next tab is missing from server layout"))?
            .id;
        let _acknowledged = self.acknowledge_active_pane_attention()?;
        Ok(())
    }
}

fn handle_focus_tab_request(
    tab_id: TabId,
    config: &ServerConfig,
    layout: &mut SessionLayout,
) -> rootcause::Result<bool> {
    let changed = layout.focus_tab(tab_id)?;
    if changed {
        crate::state::persisted::write_metadata(&config.paths, layout)?;
    }
    Ok(changed)
}

fn handle_focus_tab_request_with_tracked_process_ack(
    tab_id: TabId,
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    pane_tracked_processes: &mut PaneTrackedProcesses,
    now: Instant,
) -> rootcause::Result<bool> {
    let changed = self::handle_focus_tab_request(tab_id, config, layout)?;
    if changed {
        let _acknowledged = pane_tracked_processes.acknowledge_active_pane_attention(
            config.user_config.as_ref(),
            layout,
            runtimes,
            now,
        )?;
    }
    Ok(changed)
}

fn handle_focus_previous_tab_cmd(config: &ServerConfig, layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.focus_previous_tab()?;
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    Ok(())
}

fn handle_focus_previous_tab_cmd_with_tracked_process_ack(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    pane_tracked_processes: &mut PaneTrackedProcesses,
    now: Instant,
) -> rootcause::Result<()> {
    self::handle_focus_previous_tab_cmd(config, layout)?;
    let _acknowledged =
        pane_tracked_processes.acknowledge_active_pane_attention(config.user_config.as_ref(), layout, runtimes, now)?;
    Ok(())
}

fn handle_focus_next_tab_cmd(config: &ServerConfig, layout: &mut SessionLayout) -> rootcause::Result<()> {
    layout.focus_next_tab()?;
    crate::state::persisted::write_metadata(&config.paths, layout)?;
    Ok(())
}

fn handle_focus_next_tab_cmd_with_tracked_process_ack(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    pane_tracked_processes: &mut PaneTrackedProcesses,
    now: Instant,
) -> rootcause::Result<()> {
    self::handle_focus_next_tab_cmd(config, layout)?;
    let _acknowledged =
        pane_tracked_processes.acknowledge_active_pane_attention(config.user_config.as_ref(), layout, runtimes, now)?;
    Ok(())
}

pub fn handle_focus_tab_client_request(
    tab_id: TabId,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<TabFocusClientOutcome> {
    if state.scrollback_editor.is_some() {
        return Ok(TabFocusClientOutcome::Unchanged);
    }
    let previous_pane = state.layout.active_pane_id()?;
    if !self::handle_focus_tab_request_with_tracked_process_ack(
        tab_id,
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        Instant::now(),
    )? {
        return Ok(TabFocusClientOutcome::Unchanged);
    }
    Ok(TabFocusClientOutcome::Render { previous_pane })
}

pub fn handle_focus_previous_tab_cmd_client(
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<TabFocusClientOutcome> {
    let previous_pane = state.layout.active_pane_id()?;
    self::handle_focus_previous_tab_cmd_with_tracked_process_ack(
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        Instant::now(),
    )?;
    Ok(TabFocusClientOutcome::Render { previous_pane })
}

pub fn handle_focus_next_tab_cmd_client(
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<TabFocusClientOutcome> {
    let previous_pane = state.layout.active_pane_id()?;
    self::handle_focus_next_tab_cmd_with_tracked_process_ack(
        state.config,
        state.layout,
        state.runtimes,
        &mut state.pane_tracked_processes,
        Instant::now(),
    )?;
    Ok(TabFocusClientOutcome::Render { previous_pane })
}

#[cfg(test)]
mod tests {
    use muxr_core::PaneId;
    use muxr_core::SessionName;
    use muxr_core::TabId;

    use super::*;
    use crate::state::Pane;
    use crate::state::PaneAttentionState;
    use crate::state::PaneState;
    use crate::state::PaneTree;
    use crate::state::Tab;

    #[test]
    fn test_focus_tab_when_tab_exists_updates_active_tab() -> rootcause::Result<()> {
        let mut layout = self::layout()?;

        assert2::assert!(layout.focus_tab(TabId::new(2)?)?);

        pretty_assertions::assert_eq!(layout.active_tab.get(), 2);
        Ok(())
    }

    #[test]
    fn test_focus_tab_when_tab_is_missing_keeps_active_tab() -> rootcause::Result<()> {
        let mut layout = self::layout()?;

        assert2::assert!(!layout.focus_tab(TabId::new(3)?)?);

        pretty_assertions::assert_eq!(layout.active_tab.get(), 1);
        Ok(())
    }

    fn layout() -> rootcause::Result<SessionLayout> {
        let session: SessionName = "work".parse()?;
        let tab_1 = TabId::new(1)?;
        let tab_2 = TabId::new(2)?;
        Ok(SessionLayout {
            active_tab: tab_1,
            entries: vec![self::tab(tab_1, PaneId::new(1)?), self::tab(tab_2, PaneId::new(2)?)],
            session,
        })
    }

    fn tab(id: TabId, pane_id: PaneId) -> Tab {
        Tab {
            active_pane: pane_id,
            id,
            pane_tree: PaneTree::Pane(Pane {
                attention_state: PaneAttentionState::Idle,
                cmd_label: "sh".to_owned(),
                cwd: "/tmp".to_owned(),
                focus_seq: 1,
                id: pane_id,
                started_at: 1,
                state: PaneState::Running,
                title: "sh".to_owned(),
            }),
            title: "tab".to_owned(),
        }
    }
}
