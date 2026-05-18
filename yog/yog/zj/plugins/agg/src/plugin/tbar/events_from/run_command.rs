use std::path::PathBuf;

use crate::plugin::tbar::Event;
use crate::plugin::tbar::TbarState;

pub fn derive(state: &TbarState, requested_cwd: &PathBuf, exit_code: Option<i32>, stdout: &[u8]) -> Vec<Event> {
    if exit_code != Some(0) {
        return vec![];
    }

    let Some(current_tab) = state.current_tab.as_ref() else {
        return vec![];
    };
    let display = current_tab.current_row_display_source(state.current_tab_is_active(), &state.cwds_by_pane);
    if display.cwd.as_ref() != Some(requested_cwd) {
        return vec![];
    }

    let output = String::from_utf8_lossy(stdout);
    let Ok(records) = agg::parse_git_stat_records(&output).inspect_err(|error| eprintln!("agg: {error}")) else {
        return vec![];
    };
    for new_stat in records {
        if new_stat.path != *requested_cwd {
            continue;
        }
        if current_tab.git_stat == new_stat {
            return vec![];
        }
        return vec![Event::GitStatChanged { new_stat }];
    }

    vec![]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use agg::GitStat;
    use ytil_agents::agent::Agent;

    use crate::plugin::pane::FocusedPane;
    use crate::plugin::pane::FocusedPaneLabel;
    use crate::plugin::tbar::Event;
    use crate::plugin::tbar::current_tab::AgentPanePhase;
    use crate::plugin::tbar::current_tab::CurrentTab;
    use crate::plugin::tbar::current_tab::PaneFocus;
    use crate::plugin::tbar::events_from::run_command::*;
    use crate::plugin::tbar::test_support::*;

    #[test]
    fn test_derive_accepts_git_stat_for_inactive_winner_cwd() {
        let winner_cwd = PathBuf::from("/Users/me/codex");
        let state = state_with_inactive_winner_cwd(winner_cwd.clone());
        let new_stat = GitStat {
            path: winner_cwd.clone(),
            insertions: 1,
            ..GitStat::default()
        };

        pretty_assertions::assert_eq!(
            derive(&state, &winner_cwd, Some(0), new_stat.to_string().as_bytes()),
            vec![Event::GitStatChanged { new_stat }]
        );
    }

    #[test]
    fn test_derive_ignores_git_stat_for_previous_focused_cwd() {
        let state = state_with_inactive_winner_cwd(PathBuf::from("/Users/me/codex"));
        let focused_cwd = PathBuf::from("/Users/me/claude");
        let stale_stat = GitStat {
            path: focused_cwd.clone(),
            insertions: 1,
            ..GitStat::default()
        };

        pretty_assertions::assert_eq!(
            derive(&state, &focused_cwd, Some(0), stale_stat.to_string().as_bytes()),
            vec![]
        );
    }

    fn state_with_inactive_winner_cwd(winner_cwd: PathBuf) -> TbarState {
        TbarState {
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "current")],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                cwd_pane_id: Some(43),
                cwd: Some(PathBuf::from("/Users/me/claude")),
                pane_state_by_pane: HashMap::from([
                    (
                        42,
                        pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                    ),
                    (
                        43,
                        pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Focused, 2),
                    ),
                ]),
                ..CurrentTab::new(10)
            }),
            cwds_by_pane: HashMap::from([(42, winner_cwd), (43, PathBuf::from("/Users/me/claude"))]),
            ..Default::default()
        }
    }
}
