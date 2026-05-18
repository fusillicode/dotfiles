use crate::plugin::tbar::StateSnapshotPayload;
use crate::plugin::tbar::TbarState;
use crate::plugin::tbar::ui::TabRow;

impl TbarState {
    pub fn sync_frame(&mut self) -> bool {
        let next_frame = compute_frame(self);
        if self.frame == next_frame {
            return false;
        }
        self.frame = next_frame;
        true
    }
}

fn compute_frame(state: &TbarState) -> Vec<TabRow> {
    let current_tab_is_active = state.current_tab_is_active();
    state
        .all_tabs
        .iter()
        .map(|tab| {
            if state.current_tab_id() == Some(tab.tab_id)
                && let Some(current_tab) = state.current_tab.as_ref()
            {
                let (cmd, indicator) = current_tab.current_row_display(current_tab_is_active);
                let cwd = current_tab.display_cwd(current_tab_is_active, &state.cwds_by_pane);
                return TabRow::new(
                    tab,
                    cwd,
                    cmd,
                    indicator,
                    current_tab.git_stat.clone(),
                    state.home_dir.as_path(),
                );
            }
            if let Some(remote) = remote_snapshot_for_tab(state, tab.tab_id) {
                return TabRow::new(
                    tab,
                    remote.cwd.as_ref(),
                    remote.cmd.clone(),
                    remote.indicator,
                    remote.git_stat.clone(),
                    state.home_dir.as_path(),
                );
            }

            TabRow::placeholder(tab, state.home_dir.as_path())
        })
        .collect()
}

fn remote_snapshot_for_tab(state: &TbarState, tab_id: usize) -> Option<&StateSnapshotPayload> {
    state
        .other_tabs
        .values()
        .filter(|remote| remote.tab_id == tab_id)
        .max_by_key(|remote| remote.seq)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use agg::AgentState;
    use agg::Cmd;
    use agg::GitStat;
    use agg::TabIndicator;
    use ytil_agents::agent::Agent;
    use zellij_tile::prelude::TabInfo;

    use crate::plugin::pane::FocusedPane;
    use crate::plugin::pane::FocusedPaneLabel;
    use crate::plugin::tbar::Event;
    use crate::plugin::tbar::current_tab::AgentPanePhase;
    use crate::plugin::tbar::current_tab::CurrentTab;
    use crate::plugin::tbar::current_tab::PaneFocus;
    use crate::plugin::tbar::frame::*;
    use crate::plugin::tbar::test_support::*;

    #[test]
    fn test_compute_frame_active_mat_follows_focused_agent_when_other_pane_is_busy() {
        let mut state = TbarState {
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
        pretty_assertions::assert_eq!(
            split_events,
            vec![
                Event::PanesChanged {
                    observed_pane_ids: [42, 43].into_iter().collect(),
                    retained_pane_ids: [42, 43].into_iter().collect(),
                },
                Event::FocusChanged {
                    new_pane: Some(FocusedPane { id: 43, label: None }),
                    acknowledge_existing_attention: true,
                },
                Event::SyncRequested,
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
        pretty_assertions::assert_eq!(
            claude_events,
            vec![
                Event::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 43,
                        label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                Event::AgentDetected {
                    pane_id: 43,
                    agent: Agent::Claude,
                },
            ]
        );

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Codex, AgentState::Busy));
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Busy);

        let frame = compute_frame(&state);
        pretty_assertions::assert_eq!(
            frame,
            vec![TabRow {
                active: true,
                path_label: "-".to_string(),
                cmd: Cmd::agent(Agent::Claude, AgentState::Acknowledged),
                indicator: TabIndicator::Seen,
                git: GitStat::default(),
            }]
        );
    }

    #[test]
    fn test_compute_frame_inactive_current_tab_uses_agent_pane_cwd() {
        let state = TbarState {
            known_active_tab_id: Some(20),
            all_tabs: vec![
                tab_with_name(10, 0, "agent"),
                TabInfo {
                    active: true,
                    ..tab_with_name(20, 1, "other")
                },
            ],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("zsh".to_string())),
                }),
                cwd: Some(PathBuf::from("/Users/me/focused")),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            cwds_by_pane: HashMap::from([
                (42, PathBuf::from("/Users/me/agent")),
                (43, PathBuf::from("/Users/me/focused")),
            ]),
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let frame = compute_frame(&state);

        pretty_assertions::assert_eq!(
            frame[0],
            TabRow {
                active: false,
                path_label: "~/agent".to_string(),
                cmd: Cmd::agent(Agent::Codex, AgentState::Busy),
                indicator: TabIndicator::Busy,
                git: GitStat::default(),
            }
        );
    }

    #[test]
    fn test_compute_frame_uses_remote_indicator() {
        let state = TbarState {
            all_tabs: vec![tab_with_name(10, 0, "remote")],
            other_tabs: HashMap::from([(
                1,
                snapshot(10, 1, Cmd::Running("cargo".to_string()), TabIndicator::NoAgent),
            )]),
            ..Default::default()
        };

        let frame = compute_frame(&state);
        pretty_assertions::assert_eq!(
            frame,
            vec![TabRow {
                active: false,
                path_label: "-".to_string(),
                cmd: Cmd::Running("cargo".to_string()),
                indicator: TabIndicator::NoAgent,
                git: GitStat::default(),
            }]
        );
    }

    #[test]
    fn test_compute_frame_uses_remote_attention_indicator() {
        let state = TbarState {
            all_tabs: vec![tab_with_name(10, 0, "remote")],
            other_tabs: HashMap::from([(
                1,
                snapshot(
                    10,
                    1,
                    Cmd::agent(Agent::Codex, AgentState::NeedsAttention),
                    TabIndicator::Unseen,
                ),
            )]),
            ..Default::default()
        };

        let frame = compute_frame(&state);
        pretty_assertions::assert_eq!(
            frame,
            vec![TabRow {
                active: false,
                path_label: "-".to_string(),
                cmd: Cmd::agent(Agent::Codex, AgentState::NeedsAttention),
                indicator: TabIndicator::Unseen,
                git: GitStat::default(),
            }]
        );
    }
}
