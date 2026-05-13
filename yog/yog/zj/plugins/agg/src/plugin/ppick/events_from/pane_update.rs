use std::path::PathBuf;

use zellij_tile::prelude::PaneManifest;

use crate::plugin::ppick::state::PpickEvent;
use crate::plugin::ppick::state::PpickState;

pub fn derive(
    state: &PpickState,
    manifest: &PaneManifest,
    resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
    resolve_pane_command: impl FnMut(u32) -> Option<Vec<String>>,
) -> Vec<PpickEvent> {
    let panes = state.pane_observations(manifest, resolve_pane_cwd, resolve_pane_command);
    vec![PpickEvent::PanesUpdated { panes }]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;

    use super::*;
    use crate::plugin::ppick::ui::PpickRow;

    #[test]
    fn test_derive_pane_update_returns_event_before_apply() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![PaneInfo {
                    id: 42,
                    terminal_command: Some("codex".to_string()),
                    ..Default::default()
                }],
            ))
            .collect(),
        };

        let events = derive(
            &state,
            &manifest,
            |pane_id| (pane_id == 42).then(|| PathBuf::from("/tmp/repo")),
            |pane_id| (pane_id == 42).then(|| vec![String::from("codex")]),
        );

        assert_eq!(events.len(), 1);
        assert2::assert!(let PpickEvent::PanesUpdated { .. } = &events[0]);

        apply_events(&mut state, events);
        assert_eq!(
            state.visible_frame(usize::MAX),
            vec![PpickRow {
                selected: true,
                pane_label: "42".to_string(),
                cwd_label: "/tmp/repo".to_string(),
                branch_label: "-".to_string(),
                git: agg::GitStat::default(),
                cmd: agg::Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Acknowledged),
                indicator: agg::TabIndicator::Seen,
                session_summary: String::new(),
            }]
        );
    }

    fn apply_events(state: &mut PpickState, events: Vec<PpickEvent>) {
        for event in events {
            let _ = state.apply_event(event);
        }
    }
}
