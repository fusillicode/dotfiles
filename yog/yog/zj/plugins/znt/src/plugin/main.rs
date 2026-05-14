use std::collections::BTreeMap;

use zellij_tile::prelude::Direction;
use zellij_tile::prelude::Event;
use zellij_tile::prelude::EventType;
use zellij_tile::prelude::PermissionStatus;
use zellij_tile::prelude::PermissionType;
use zellij_tile::prelude::PipeMessage;
use zellij_tile::prelude::TabInfo;
use zellij_tile::prelude::ZellijPlugin;
use zellij_tile::prelude::actions::Action;

const CONTEXT_KIND: &str = "kind";
const CONTEXT_MOVE_TAB: &str = "znt-move-tab";
const ZNT_PIPE: &str = "znt";

#[derive(Default)]
pub struct State {
    tabs: Vec<TabInfo>,
    pending_create: bool,
    pending_moves: usize,
}

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        zellij_tile::prelude::request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunActionsAsUser,
        ]);
        zellij_tile::prelude::subscribe(&[EventType::PermissionRequestResult]);
    }

    fn update(&mut self, event: Event) -> bool {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "znt only subscribes to permission, tab, and action events"
        )]
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                zellij_tile::prelude::subscribe(&[EventType::TabUpdate, EventType::ActionComplete]);
                zellij_tile::prelude::set_selectable(false);
            }
            Event::TabUpdate(tabs) => self.update_tabs(tabs),
            Event::ActionComplete(_action, _pane_id, context)
                if context.get(CONTEXT_KIND).is_some_and(|kind| kind == CONTEXT_MOVE_TAB)
                    && self.pending_moves != 0 =>
            {
                self.move_next();
            }
            _ => {}
        }
        false
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        if pipe_message.name == ZNT_PIPE {
            self.request_create();
        }
        false
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
enum CreateRequest {
    Start { moves: usize },
    WaitForTabUpdate,
}

impl State {
    fn update_tabs(&mut self, tabs: Vec<TabInfo>) {
        self.tabs = tabs;
        if self.pending_create && self.pending_moves == 0 {
            self.pending_create = false;
            self.request_create();
        }
    }

    fn request_create(&mut self) {
        if self.pending_moves != 0 {
            return;
        }

        match create_request(&self.tabs) {
            CreateRequest::Start { moves } => self.create_tab(moves),
            CreateRequest::WaitForTabUpdate => self.pending_create = true,
        }
    }

    fn create_tab(&mut self, moves: usize) {
        if zellij_tile::prelude::new_tab::<&str>(None, None).is_none() {
            return;
        }
        self.pending_moves = moves;
        self.move_next();
    }

    fn move_next(&mut self) {
        let Some(remaining_moves) = self.pending_moves.checked_sub(1) else {
            return;
        };
        self.pending_moves = remaining_moves;

        let mut context = BTreeMap::new();
        context.insert(CONTEXT_KIND.to_string(), CONTEXT_MOVE_TAB.to_string());
        zellij_tile::prelude::run_action(
            Action::MoveTab {
                direction: Direction::Left,
            },
            context,
        );
    }
}

fn create_request(tabs: &[TabInfo]) -> CreateRequest {
    let Some(active_tab) = tabs.iter().find(|tab| tab.active) else {
        return CreateRequest::WaitForTabUpdate;
    };
    let active_next_position = active_tab.position.saturating_add(1);
    CreateRequest::Start {
        moves: tabs.len().saturating_sub(active_next_position),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
const extern "C" fn host_run_plugin_command() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_request_first_tab_moves_new_tab_next_to_first_tab() {
        let tabs = vec![tab(0, true), tab(1, false), tab(2, false)];

        pretty_assertions::assert_eq!(create_request(&tabs), CreateRequest::Start { moves: 2 });
    }

    #[test]
    fn test_create_request_middle_tab_moves_new_tab_next_to_middle_tab() {
        let tabs = vec![tab(0, false), tab(1, true), tab(2, false), tab(3, false)];

        pretty_assertions::assert_eq!(create_request(&tabs), CreateRequest::Start { moves: 2 });
    }

    #[test]
    fn test_create_request_last_tab_keeps_new_tab_at_end() {
        let tabs = vec![tab(0, false), tab(1, false), tab(2, true)];

        pretty_assertions::assert_eq!(create_request(&tabs), CreateRequest::Start { moves: 0 });
    }

    #[test]
    fn test_create_request_without_active_tab_waits_for_tab_update() {
        let tabs = vec![tab(0, false), tab(1, false)];

        pretty_assertions::assert_eq!(create_request(&tabs), CreateRequest::WaitForTabUpdate);
    }

    fn tab(position: usize, active: bool) -> TabInfo {
        TabInfo {
            position,
            active,
            ..TabInfo::default()
        }
    }
}
