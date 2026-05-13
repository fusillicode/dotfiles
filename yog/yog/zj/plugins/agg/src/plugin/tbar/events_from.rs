use crate::plugin::pane::FocusedPane;
use crate::plugin::tbar::Event;

pub mod active_tab;
pub mod agent;
pub mod cwd;
pub mod pane_close;
pub mod pane_update;
pub mod run_command;
pub mod snapshot;
pub mod tab_update;

fn push_became_active(events: &mut Vec<Event>, landing_focus: Option<FocusedPane>) {
    events.push(Event::BecameActive);
    if let Some(focused_pane) = landing_focus {
        events.push(Event::FocusChanged {
            new_pane: Some(focused_pane),
            acknowledge_existing_attention: true,
        });
    }
}
