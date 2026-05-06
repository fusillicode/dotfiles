use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use current_tab::CurrentTab;
use zellij_tile::prelude::TabInfo;

use crate::wasm::plugin::StateSnapshotPayload;
use crate::wasm::ui::TabRow;

mod apply;
pub mod current_tab;
mod events_from;
mod frame;
pub mod nudge;
pub mod pane;
mod tabs;

#[derive(Default)]
pub struct State {
    pub plugin_id: u32,
    pub all_tabs: Vec<TabInfo>,
    pub current_tab: Option<CurrentTab>,
    pub other_tabs: HashMap<u32, StateSnapshotPayload>,
    pub known_active_tab_id: Option<usize>,
    pub sync_requested: bool,
    pub nudged_pane_ids: HashSet<u32>,
    pub home_dir: PathBuf,
    pub zellij_session_name: Option<String>,
    pub frame: Vec<TabRow>,
    pub last_cols: usize,
    pub render_buf: String,
}

#[cfg(test)]
mod test_support {
    use std::path::PathBuf;

    use agg::Cmd;
    use agg::GitStat;
    use agg::TabIndicator;
    use ytil_agents::agent::Agent;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;
    use zellij_tile::prelude::TabInfo;

    use super::State;
    use super::current_tab::AgentPanePhase;
    use super::current_tab::AgentPaneState;
    use super::current_tab::PaneFocus;
    use crate::wasm::events::StateEvent;
    use crate::wasm::plugin::StateSnapshotPayload;

    pub fn noop_pane_cwd(_pane_id: u32) -> Option<PathBuf> {
        None
    }

    pub fn tab_with_name(tab_id: usize, position: usize, name: &str) -> TabInfo {
        TabInfo {
            tab_id,
            position,
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn plugin_pane(id: u32) -> PaneInfo {
        PaneInfo {
            id,
            is_plugin: true,
            ..Default::default()
        }
    }

    pub fn terminal_pane_with_command(id: u32, is_focused: bool, command: &str) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            terminal_command: Some(command.to_string()),
            ..Default::default()
        }
    }

    pub fn terminal_pane_with_title(id: u32, is_focused: bool, title: &str) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            title: title.to_string(),
            ..Default::default()
        }
    }

    pub fn manifest(entries: Vec<(usize, Vec<PaneInfo>)>) -> PaneManifest {
        PaneManifest {
            panes: entries.into_iter().collect(),
        }
    }

    pub fn pane_state(agent: Agent, phase: AgentPanePhase, focus: PaneFocus, phase_seq: u64) -> AgentPaneState {
        AgentPaneState {
            agent,
            phase,
            focus,
            phase_seq,
        }
    }

    pub fn snapshot(tab_id: usize, seq: u64, cmd: Cmd, indicator: TabIndicator) -> StateSnapshotPayload {
        StateSnapshotPayload {
            tab_id,
            seq,
            cwd: None,
            cmd,
            indicator,
            git_stat: GitStat::default(),
        }
    }

    pub fn apply_pane_update(state: &mut State, manifest: &PaneManifest) -> Vec<StateEvent> {
        let events = state.events_from_pane_update(manifest, noop_pane_cwd);
        let _ = state.apply_all(&events);
        events
    }
}
