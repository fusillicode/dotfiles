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
    if current_tab.display_cwd(state.current_tab_is_active(), &state.cwds_by_pane) != Some(requested_cwd) {
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
    use crate::plugin::tbar::TbarState;
    use crate::plugin::tbar::current_tab::AgentPanePhase;
    use crate::plugin::tbar::current_tab::CurrentTab;
    use crate::plugin::tbar::current_tab::PaneFocus;
    use crate::plugin::tbar::events_from::run_command::*;
    use crate::plugin::tbar::test_support::pane_state;

    #[test]
    fn test_derive_accepts_git_stat_for_inactive_display_cwd() {
        let requested_cwd = PathBuf::from("/Users/me/agent");
        let state = TbarState {
            known_active_tab_id: Some(20),
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
            cwds_by_pane: HashMap::from([(42, requested_cwd.clone()), (43, PathBuf::from("/Users/me/focused"))]),
            ..Default::default()
        };
        let new_stat = GitStat {
            path: requested_cwd.clone(),
            insertions: 3,
            ..Default::default()
        };
        let stdout = new_stat.to_string();

        pretty_assertions::assert_eq!(
            derive(&state, &requested_cwd, Some(0), stdout.as_bytes()),
            vec![Event::GitStatChanged { new_stat }]
        );
    }
}
