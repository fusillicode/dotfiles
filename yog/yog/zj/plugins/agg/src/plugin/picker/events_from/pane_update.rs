use std::path::PathBuf;

use zellij_tile::prelude::PaneManifest;

use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;

pub fn derive(
    state: &PickerState,
    manifest: &PaneManifest,
    resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
    resolve_pane_command: impl FnMut(u32) -> Option<Vec<String>>,
) -> Vec<PickerEvent> {
    let panes = state.pane_observations(manifest, resolve_pane_cwd, resolve_pane_command);
    vec![PickerEvent::PanesUpdated { panes }]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;

    use super::*;
    use crate::plugin::picker::ui::PickerRow;

    #[test]
    fn test_derive_pane_update_returns_event_before_apply() {
        let mut state = PickerState::default();
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
        assert2::assert!(let PickerEvent::PanesUpdated { .. } = &events[0]);

        apply_events(&mut state, events);
        assert_eq!(
            state.visible_frame(usize::MAX),
            vec![PickerRow {
                selected: true,
                cwd_label: "/tmp/repo".to_string(),
                branch_label: "-".to_string(),
                git: agg::GitStat::default(),
                cmd: agg::Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Acknowledged),
                indicator: agg::TabIndicator::Seen,
                session_summary: String::new(),
            }]
        );
    }

    fn apply_events(state: &mut PickerState, events: Vec<PickerEvent>) {
        for event in events {
            let _ = state.apply_event(event);
        }
    }
}
