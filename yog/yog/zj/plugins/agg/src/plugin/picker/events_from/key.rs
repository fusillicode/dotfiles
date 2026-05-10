use zellij_tile::prelude::KeyWithModifier;

use crate::plugin::picker::state::PickerAction;
use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;

pub fn derive(state: &PickerState, key: &KeyWithModifier) -> (Vec<PickerEvent>, PickerAction) {
    let (event, action) = state.derive_key_event(key);
    let events = event.map_or_else(Vec::new, |event| {
        crate::plugin::picker::events_from::picker_event(state, event)
    });
    (events, action)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pretty_assertions::assert_eq;
    use zellij_tile::prelude::BareKey;
    use zellij_tile::prelude::KeyModifier;
    use zellij_tile::prelude::KeyWithModifier;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;

    use crate::plugin::picker::events_from::key::*;

    #[test]
    fn test_derive_key_returns_query_event_and_focus_action_separately() {
        let mut state = PickerState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane(42, "cargo")])).collect(),
        };
        let events = crate::plugin::picker::events_from::pane_update::derive(&state, &manifest, |_| None, |_| None);
        apply_events(&mut state, &events);

        let (events, action) = derive(&state, &KeyWithModifier::new(BareKey::Char('c')));

        assert_eq!(action, PickerAction::Redraw);
        assert_eq!(events.len(), 1);
        assert_eq!(state.frame().len(), 1);

        apply_events(&mut state, &events);
        let (events, action) = derive(&state, &KeyWithModifier::new(BareKey::Enter));

        assert_eq!(events, vec![]);
        assert_eq!(action, PickerAction::Focus(42));
    }

    #[test]
    fn test_derive_key_selection_returns_selection_event() {
        let mut state = PickerState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane(42, "cargo"), terminal_pane(43, "nvim")])).collect(),
        };
        let events = crate::plugin::picker::events_from::pane_update::derive(&state, &manifest, |_| None, |_| None);
        apply_events(&mut state, &events);
        let ctrl_n = KeyWithModifier::new_with_modifiers(BareKey::Char('n'), BTreeSet::from([KeyModifier::Ctrl]));

        let (events, action) = derive(&state, &ctrl_n);

        assert_eq!(action, PickerAction::Redraw);
        assert_eq!(events.len(), 1);
    }

    fn apply_events(state: &mut PickerState, events: &[PickerEvent]) {
        for event in events {
            let _ = state.apply_event(event);
        }
    }

    fn terminal_pane(id: u32, command: &str) -> PaneInfo {
        PaneInfo {
            id,
            terminal_command: Some(command.to_string()),
            ..Default::default()
        }
    }
}
