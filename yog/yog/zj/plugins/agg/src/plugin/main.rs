use std::collections::BTreeMap;
use std::path::PathBuf;

use zellij_tile::prelude::Event;
use zellij_tile::prelude::EventType;
use zellij_tile::prelude::Mouse;
use zellij_tile::prelude::PaneId;
use zellij_tile::prelude::PermissionStatus;
use zellij_tile::prelude::PermissionType;
use zellij_tile::prelude::PipeMessage;
use zellij_tile::prelude::ZellijPlugin;

use crate::plugin::ppick::state::PpickState;
use crate::plugin::tbar::TbarState;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Clone, Copy, Default)]
pub enum Component {
    #[default]
    Tbar,
    Ppick,
}

impl From<&BTreeMap<String, String>> for Component {
    fn from(config: &BTreeMap<String, String>) -> Self {
        if config.get("component").is_some_and(|value| value == "ppick") {
            Self::Ppick
        } else {
            Self::Tbar
        }
    }
}

pub enum ComponentState {
    Tbar(Box<TbarState>),
    Ppick(Box<PpickState>),
}

impl Default for ComponentState {
    fn default() -> Self {
        Self::Tbar(Box::default())
    }
}

impl From<Component> for ComponentState {
    fn from(value: Component) -> Self {
        match value {
            Component::Tbar => Self::Tbar(Box::default()),
            Component::Ppick => Self::Ppick(Box::default()),
        }
    }
}

#[derive(Default)]
pub struct State {
    pub component: ComponentState,
    pub last_cols: usize,
    pub render_buf: String,
}

impl ZellijPlugin for State {
    fn load(&mut self, config: BTreeMap<String, String>) {
        self.component = ComponentState::from(Component::from(&config));
        let home_dir = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        match &mut self.component {
            ComponentState::Tbar(tbar) => crate::plugin::tbar::load(tbar, home_dir),
            ComponentState::Ppick(ppick) => crate::plugin::ppick::load(ppick, home_dir, &config),
        }
        zellij_tile::prelude::request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
            PermissionType::MessageAndLaunchOtherPlugins,
            PermissionType::ReadSessionEnvironmentVariables,
        ]);
        zellij_tile::prelude::subscribe(&[EventType::PermissionRequestResult]);
    }

    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "agg only subscribes to a small Event subset"
    )]
    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => match &mut self.component {
                ComponentState::Tbar(tbar) => crate::plugin::tbar::update_permission_granted(tbar),
                ComponentState::Ppick(ppick) => crate::plugin::ppick::update_permission_granted(ppick),
            },
            Event::TabUpdate(tabs) => match &mut self.component {
                ComponentState::Tbar(tbar) => crate::plugin::tbar::update_tabs(tbar, tabs),
                ComponentState::Ppick(ppick) => crate::plugin::ppick::update_tabs(ppick, tabs),
            },
            Event::PaneUpdate(manifest) => match &mut self.component {
                ComponentState::Tbar(tbar) => crate::plugin::tbar::update_panes(tbar, &manifest),
                ComponentState::Ppick(ppick) => crate::plugin::ppick::update_panes(ppick, &manifest),
            },
            Event::PaneClosed(PaneId::Terminal(pane_id)) => match &mut self.component {
                ComponentState::Tbar(tbar) => crate::plugin::tbar::update_pane_closed(tbar, pane_id),
                ComponentState::Ppick(ppick) => crate::plugin::ppick::update_pane_closed(ppick, pane_id),
            },
            Event::CwdChanged(PaneId::Terminal(pane_id), cwd, _clients) => match &mut self.component {
                ComponentState::Tbar(tbar) => crate::plugin::tbar::update_cwd(tbar, pane_id, cwd),
                ComponentState::Ppick(ppick) => crate::plugin::ppick::update_cwd(ppick, pane_id, &cwd),
            },
            Event::CommandChanged(pane_id, command, is_foreground, _clients) => match &mut self.component {
                ComponentState::Tbar(_) => false,
                ComponentState::Ppick(ppick) => {
                    crate::plugin::ppick::update_command(ppick, pane_id, &command, is_foreground)
                }
            },
            Event::Key(key) => match &mut self.component {
                ComponentState::Tbar(_) => false,
                ComponentState::Ppick(ppick) => crate::plugin::ppick::update_key(ppick, &key),
            },
            Event::RunCommandResult(exit_code, stdout, stderr, context) => match &mut self.component {
                ComponentState::Tbar(tbar) => {
                    crate::plugin::tbar::update_run_command_result(tbar, exit_code, &stdout, &context)
                }
                ComponentState::Ppick(ppick) => {
                    crate::plugin::ppick::update_run_command_result(ppick, exit_code, &stdout, &stderr, &context)
                }
            },
            Event::Mouse(Mouse::LeftClick(row, _col)) => match &self.component {
                ComponentState::Tbar(tbar) => crate::plugin::tbar::update_mouse_left_click(tbar, row, self.last_cols),
                ComponentState::Ppick(_) => false,
            },
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.last_cols = cols;
        self.render_buf.clear();
        match &mut self.component {
            ComponentState::Tbar(tbar) => {
                crate::plugin::tbar::render(tbar, rows, cols, &mut self.render_buf);
            }
            ComponentState::Ppick(ppick) => {
                crate::plugin::ppick::render(ppick, rows, cols, &mut self.render_buf);
            }
        }
        if !self.render_buf.is_empty() {
            print!("{}", self.render_buf);
        }
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        match &mut self.component {
            ComponentState::Tbar(tbar) => crate::plugin::tbar::pipe(tbar, &pipe_message),
            ComponentState::Ppick(ppick) => crate::plugin::ppick::pipe(ppick, &pipe_message),
        }
    }
}

// No-op symbol for tests builds so unit tests can link/run in CI.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
const extern "C" fn host_run_plugin_command() {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zellij_tile::prelude::PipeMessage;
    use zellij_tile::prelude::PipeSource;
    use zellij_tile::prelude::ZellijPlugin;

    use crate::plugin::main::Component;
    use crate::plugin::main::ComponentState;
    use crate::plugin::main::State;
    use crate::plugin::tbar::AGG_SYNC_PIPE;

    #[test]
    fn test_component_from_defaults_to_tbar_for_missing_or_unknown_component() {
        assert_eq!(Component::from(&BTreeMap::new()), Component::Tbar);
        assert_eq!(
            Component::from(&BTreeMap::from([(String::from("component"), String::from("unknown"),)])),
            Component::Tbar
        );
    }

    #[test]
    fn test_component_from_selects_ppick_for_ppick_component() {
        assert_eq!(
            Component::from(&BTreeMap::from([(String::from("component"), String::from("ppick"))])),
            Component::Ppick
        );
    }

    #[test]
    fn test_pipe_returns_false_for_ppick_component() {
        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "sync_request".to_string());
        let msg = PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGG_SYNC_PIPE.to_string(),
            payload: None,
            args,
            is_private: false,
        };
        let mut state = State {
            component: ComponentState::Ppick(Box::default()),
            ..Default::default()
        };

        assert2::assert!(!state.pipe(msg));
    }
}
