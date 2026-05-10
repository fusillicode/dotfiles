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
    crate::plugin::picker::events_from::picker_event(state, PickerEvent::PanesUpdated { panes })
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
        state.home_dir = PathBuf::from("/tmp");
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

        apply_events(&mut state, &events);
        assert_eq!(
            state.frame(),
            vec![PickerRow {
                selected: true,
                cwd_label: "~/repo".to_string(),
                summary: String::new(),
                label: "cx".to_string(),
                marker: agg::TabIndicator::Seen,
                git: agg::GitStat::default(),
            }]
        );
    }

    fn apply_events(state: &mut PickerState, events: &[PickerEvent]) {
        for event in events {
            let _ = state.apply_event(event);
        }
    }
}
