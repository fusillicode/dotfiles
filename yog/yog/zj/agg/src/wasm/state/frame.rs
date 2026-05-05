use agg::Cmd;
use agg::GitStat;
use agg::TabIndicator;

use super::State;
use crate::wasm::plugin::StateSnapshotPayload;
use crate::wasm::ui::TabRow;

impl State {
    pub fn remote_snapshot_for_tab(&self, tab_id: usize) -> Option<&StateSnapshotPayload> {
        self.other_tabs
            .values()
            .filter(|remote| remote.tab_id == tab_id)
            .max_by_key(|remote| remote.seq)
    }

    pub fn sync_frame(&mut self) -> bool {
        let next_frame = compute_frame(self);
        if self.frame == next_frame {
            return false;
        }
        self.frame = next_frame;
        true
    }
}

fn compute_frame(state: &State) -> Vec<TabRow> {
    let current_tab_is_active = state.current_tab_is_active();
    state
        .all_tabs
        .iter()
        .map(|tab| {
            if state.current_tab_id() == Some(tab.tab_id)
                && let Some(current_tab) = state.current_tab.as_ref()
            {
                let (cmd, indicator) = current_tab.current_row_display(current_tab_is_active);
                return TabRow::new(
                    tab,
                    current_tab.cwd.as_ref(),
                    cmd,
                    indicator,
                    current_tab.git_stat,
                    state.home_dir.as_path(),
                );
            }
            if let Some(remote) = state.remote_snapshot_for_tab(tab.tab_id) {
                return TabRow::new(
                    tab,
                    remote.cwd.as_ref(),
                    remote.cmd.clone(),
                    remote.indicator,
                    remote.git_stat,
                    state.home_dir.as_path(),
                );
            }

            TabRow::new(
                tab,
                None,
                Cmd::None,
                TabIndicator::None,
                GitStat::default(),
                state.home_dir.as_path(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use agg::AgentState;
    use assert2::assert;
    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;
    use zellij_tile::prelude::TabInfo;

    use super::*;
    use crate::wasm::events::StateEvent;
    use crate::wasm::state::current_tab::AgentPanePhase;
    use crate::wasm::state::current_tab::CurrentTab;
    use crate::wasm::state::current_tab::FocusedPane;
    use crate::wasm::state::current_tab::FocusedPaneLabel;
    use crate::wasm::state::current_tab::PaneFocus;
    use crate::wasm::state::test_support::*;

    #[test]
    fn test_compute_frame_active_mat_follows_focused_agent_when_other_pane_is_green() {
        let mut state = State {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![TabInfo {
                active: true,
                ..tab_with_name(10, 0, "a")
            }],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let split_events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![
                    plugin_pane(7),
                    terminal_pane_with_command(42, false, "codex"),
                    terminal_pane_with_command(43, true, "/bin/zsh"),
                ],
            )]),
        );
        assert_eq!(
            split_events,
            vec![
                StateEvent::PanesChanged {
                    observed_pane_ids: [42, 43].into_iter().collect(),
                    retained_pane_ids: [42, 43].into_iter().collect(),
                },
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane { id: 43, label: None }),
                    acknowledge_existing_attention: true,
                },
                StateEvent::SyncRequested,
            ]
        );

        let claude_events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![
                    plugin_pane(7),
                    terminal_pane_with_command(42, false, "codex"),
                    terminal_pane_with_command(43, true, "claude"),
                ],
            )]),
        );
        assert_eq!(
            claude_events,
            vec![
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 43,
                        label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                StateEvent::AgentDetected {
                    pane_id: 43,
                    agent: Agent::Claude,
                },
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Codex, AgentState::Busy));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Green);

        let frame = compute_frame(&state);
        assert_eq!(
            frame,
            vec![TabRow {
                active: true,
                path_label: "a".to_string(),
                cmd: Cmd::agent(Agent::Claude, AgentState::Acknowledged),
                indicator: TabIndicator::Empty,
                git: GitStat::default(),
            }]
        );
    }

    #[test]
    fn test_compute_frame_uses_remote_indicator() {
        let state = State {
            all_tabs: vec![tab_with_name(10, 0, "remote")],
            other_tabs: HashMap::from([(
                1,
                snapshot(10, 1, Cmd::Running("cargo".to_string()), TabIndicator::None),
            )]),
            ..Default::default()
        };

        let frame = compute_frame(&state);
        assert_eq!(
            frame,
            vec![TabRow {
                active: false,
                path_label: "remote".to_string(),
                cmd: Cmd::Running("cargo".to_string()),
                indicator: TabIndicator::None,
                git: GitStat::default(),
            }]
        );
    }

    #[test]
    fn test_compute_frame_uses_remote_attention_indicator() {
        let state = State {
            all_tabs: vec![tab_with_name(10, 0, "remote")],
            other_tabs: HashMap::from([(
                1,
                snapshot(
                    10,
                    1,
                    Cmd::agent(Agent::Codex, AgentState::NeedsAttention),
                    TabIndicator::Red,
                ),
            )]),
            ..Default::default()
        };

        let frame = compute_frame(&state);
        assert_eq!(
            frame,
            vec![TabRow {
                active: false,
                path_label: "remote".to_string(),
                cmd: Cmd::agent(Agent::Codex, AgentState::NeedsAttention),
                indicator: TabIndicator::Red,
                git: GitStat::default(),
            }]
        );
    }
}
