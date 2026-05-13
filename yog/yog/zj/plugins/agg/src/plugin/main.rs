use std::collections::BTreeMap;
use std::path::PathBuf;

use zellij_tile::prelude::Event;
use zellij_tile::prelude::EventType;
use zellij_tile::prelude::KeyWithModifier;
use zellij_tile::prelude::Mouse;
use zellij_tile::prelude::PaneId;
use zellij_tile::prelude::PaneManifest;
use zellij_tile::prelude::PermissionStatus;
use zellij_tile::prelude::PermissionType;
use zellij_tile::prelude::PipeMessage;
use zellij_tile::prelude::TabInfo;
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

impl State {
    fn update_permission_granted(&mut self) -> bool {
        match &mut self.component {
            ComponentState::Tbar(tbar) => crate::plugin::tbar::update_permission_granted(tbar),
            ComponentState::Ppick(ppick) => crate::plugin::ppick::update_permission_granted(ppick),
        }
    }

    fn update_tabs(&mut self, tabs: Vec<TabInfo>) -> bool {
        match &mut self.component {
            ComponentState::Tbar(tbar) => crate::plugin::tbar::update_tabs(tbar, tabs),
            ComponentState::Ppick(ppick) => crate::plugin::ppick::update_tabs(ppick, tabs),
        }
    }

    fn update_panes(&mut self, manifest: &PaneManifest) -> bool {
        match &mut self.component {
            ComponentState::Tbar(tbar) => crate::plugin::tbar::update_panes(tbar, manifest),
            ComponentState::Ppick(ppick) => crate::plugin::ppick::update_panes(ppick, manifest),
        }
    }

    fn update_pane_closed(&mut self, pane_id: u32) -> bool {
        match &mut self.component {
            ComponentState::Tbar(tbar) => crate::plugin::tbar::update_pane_closed(tbar, pane_id),
            ComponentState::Ppick(ppick) => crate::plugin::ppick::update_pane_closed(ppick, pane_id),
        }
    }

    fn update_cwd(&mut self, pane_id: u32, cwd: PathBuf) -> bool {
        match &mut self.component {
            ComponentState::Tbar(tbar) => crate::plugin::tbar::update_cwd(tbar, pane_id, cwd),
            ComponentState::Ppick(ppick) => crate::plugin::ppick::update_cwd(ppick, pane_id, cwd),
        }
    }

    fn update_command(&mut self, pane_id: PaneId, command: &[String], is_foreground: bool) -> bool {
        let ComponentState::Ppick(ppick) = &mut self.component else {
            return false;
        };
        crate::plugin::ppick::update_command(ppick, pane_id, command, is_foreground)
    }

    fn update_key(&mut self, key: &KeyWithModifier) -> bool {
        let ComponentState::Ppick(ppick) = &mut self.component else {
            return false;
        };
        crate::plugin::ppick::update_key(ppick, key)
    }

    fn update_run_command_result(
        &mut self,
        exit_code: Option<i32>,
        stdout: &[u8],
        stderr: &[u8],
        context: &BTreeMap<String, String>,
    ) -> bool {
        match &mut self.component {
            ComponentState::Tbar(tbar) => {
                crate::plugin::tbar::update_run_command_result(tbar, exit_code, stdout, context)
            }
            ComponentState::Ppick(ppick) => {
                crate::plugin::ppick::update_run_command_result(ppick, exit_code, stdout, stderr, context)
            }
        }
    }

    fn update_mouse_left_click(&self, row: isize) -> bool {
        let ComponentState::Tbar(tbar) = &self.component else {
            return false;
        };
        crate::plugin::tbar::update_mouse_left_click(tbar, row, self.last_cols)
    }
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

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => self.update_permission_granted(),
            Event::TabUpdate(tabs) => self.update_tabs(tabs),
            Event::PaneUpdate(manifest) => self.update_panes(&manifest),
            Event::PaneClosed(PaneId::Terminal(pane_id)) => self.update_pane_closed(pane_id),
            Event::CwdChanged(PaneId::Terminal(pane_id), cwd, _clients) => self.update_cwd(pane_id, cwd),
            Event::CommandChanged(pane_id, command, is_foreground, _clients) => {
                self.update_command(pane_id, &command, is_foreground)
            }
            Event::Key(key) => self.update_key(&key),
            Event::RunCommandResult(exit_code, stdout, stderr, context) => {
                self.update_run_command_result(exit_code, &stdout, &stderr, &context)
            }
            Event::Mouse(Mouse::LeftClick(row, _col)) => self.update_mouse_left_click(row),
            Event::ModeUpdate(_)
            | Event::Mouse(_)
            | Event::Timer(_)
            | Event::CopyToClipboard(_)
            | Event::SystemClipboardFailure
            | Event::InputReceived
            | Event::Visible(_)
            | Event::CustomMessage(..)
            | Event::FileSystemCreate(_)
            | Event::FileSystemRead(_)
            | Event::FileSystemUpdate(_)
            | Event::FileSystemDelete(_)
            | Event::PermissionRequestResult(_)
            | Event::SessionUpdate(..)
            | Event::WebRequestResult(..)
            | Event::CommandPaneOpened(..)
            | Event::CommandPaneExited(..)
            | Event::PaneClosed(_)
            | Event::EditPaneOpened(..)
            | Event::EditPaneExited(..)
            | Event::CommandPaneReRun(..)
            | Event::FailedToWriteConfigToDisk(_)
            | Event::ListClients(_)
            | Event::HostFolderChanged(_)
            | Event::FailedToChangeHostFolder(_)
            | Event::PastedText(_)
            | Event::ConfigWasWrittenToDisk
            | Event::WebServerStatus(_)
            | Event::FailedToStartWebServer(_)
            | Event::BeforeClose
            | Event::InterceptedKeyPress(_)
            | Event::UserAction(..)
            | Event::PaneRenderReport(_)
            | Event::PaneRenderReportWithAnsi(_)
            | Event::ActionComplete(..)
            | Event::CwdChanged(..)
            | Event::AvailableLayoutInfo(..)
            | Event::PluginConfigurationChanged(_)
            | Event::HighlightClicked { .. }
            | Event::InitialKeybinds(_)
            | _ => false,
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
