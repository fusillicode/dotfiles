use std::path::PathBuf;

use crate::plugin::tbar::Event;
use crate::plugin::tbar::TbarState;

pub fn derive(state: &TbarState, pane_id: u32, cwd: PathBuf) -> Vec<Event> {
    let display_cwd_changed = state.current_tab.as_ref().is_some_and(|current_tab| {
        current_tab.focused_pane.as_ref().map(|focused_pane| focused_pane.id) == Some(pane_id)
            && current_tab.cwd.as_ref() != Some(&cwd)
    });
    if state.cwds_by_pane.get(&pane_id) == Some(&cwd) && !display_cwd_changed {
        return vec![];
    }
    vec![Event::CwdChanged { pane_id, new_cwd: cwd }]
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use crate::plugin::tbar::Event;
    use crate::plugin::tbar::TbarState;
    use crate::plugin::tbar::events_from::cwd::*;

    #[test]
    fn test_derive_caches_non_display_pane_cwd() {
        assert_eq!(
            derive(&TbarState::default(), 42, PathBuf::from("/tmp/project")),
            vec![Event::CwdChanged {
                pane_id: 42,
                new_cwd: PathBuf::from("/tmp/project"),
            }]
        );
    }

    #[test]
    fn test_derive_skips_unchanged_cached_cwd() {
        let state = TbarState {
            cwds_by_pane: HashMap::from([(42, PathBuf::from("/tmp/project"))]),
            ..Default::default()
        };

        assert_eq!(derive(&state, 42, PathBuf::from("/tmp/project")), vec![]);
    }
}
