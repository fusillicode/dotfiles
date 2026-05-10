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

use crate::plugin::picker::state::PickerState;
use crate::plugin::tab_bar::TabBarState;

#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub enum View {
    #[default]
    TabBar,
    Picker,
}

impl From<&BTreeMap<String, String>> for View {
    fn from(config: &BTreeMap<String, String>) -> Self {
        if config.get("view").is_some_and(|value| value == "picker") {
            Self::Picker
        } else {
            Self::TabBar
        }
    }
}

pub enum ViewState {
    TabBar(Box<TabBarState>),
    Picker(Box<PickerState>),
}

impl Default for ViewState {
    fn default() -> Self {
        Self::TabBar(Box::default())
    }
}

impl From<View> for ViewState {
    fn from(value: View) -> Self {
        match value {
            View::TabBar => Self::TabBar(Box::default()),
            View::Picker => Self::Picker(Box::default()),
        }
    }
}

#[derive(Default)]
pub struct State {
    pub view: ViewState,
    pub last_cols: usize,
    pub render_buf: String,
}

impl State {
    fn update_permission_granted(&mut self) -> bool {
        match &mut self.view {
            ViewState::TabBar(tab_bar) => crate::plugin::tab_bar::update_permission_granted(tab_bar),
            ViewState::Picker(picker) => crate::plugin::picker::update_permission_granted(picker),
        }
    }

    fn update_tabs(&mut self, tabs: Vec<TabInfo>) -> bool {
        match &mut self.view {
            ViewState::TabBar(tab_bar) => crate::plugin::tab_bar::update_tabs(tab_bar, tabs),
            ViewState::Picker(picker) => crate::plugin::picker::update_tabs(picker, tabs),
        }
    }

    fn update_panes(&mut self, manifest: &PaneManifest) -> bool {
        match &mut self.view {
            ViewState::TabBar(tab_bar) => crate::plugin::tab_bar::update_panes(tab_bar, manifest),
            ViewState::Picker(picker) => crate::plugin::picker::update_panes(picker, manifest),
        }
    }

    fn update_pane_closed(&mut self, pane_id: u32) -> bool {
        match &mut self.view {
            ViewState::TabBar(tab_bar) => crate::plugin::tab_bar::update_pane_closed(tab_bar, pane_id),
            ViewState::Picker(picker) => crate::plugin::picker::update_pane_closed(picker, pane_id),
        }
    }

    fn update_cwd(&mut self, pane_id: u32, cwd: PathBuf) -> bool {
        match &mut self.view {
            ViewState::TabBar(tab_bar) => crate::plugin::tab_bar::update_cwd(tab_bar, pane_id, cwd),
            ViewState::Picker(picker) => crate::plugin::picker::update_cwd(picker, pane_id, cwd),
        }
    }

    fn update_command(&mut self, pane_id: PaneId, command: &[String], is_foreground: bool) -> bool {
        let ViewState::Picker(picker) = &mut self.view else {
            return false;
        };
        crate::plugin::picker::update_command(picker, pane_id, command, is_foreground)
    }

    fn update_key(&mut self, key: &KeyWithModifier) -> bool {
        let ViewState::Picker(picker) = &mut self.view else {
            return false;
        };
        crate::plugin::picker::update_key(picker, key)
    }

    fn update_run_command_result(
        &mut self,
        exit_code: Option<i32>,
        stdout: &[u8],
        stderr: &[u8],
        context: &BTreeMap<String, String>,
    ) -> bool {
        match &mut self.view {
            ViewState::TabBar(tab_bar) => {
                crate::plugin::tab_bar::update_run_command_result(tab_bar, exit_code, stdout, context)
            }
            ViewState::Picker(picker) => {
                crate::plugin::picker::update_run_command_result(picker, exit_code, stdout, stderr, context)
            }
        }
    }

    fn update_mouse_left_click(&self, row: isize) -> bool {
        let ViewState::TabBar(tab_bar) = &self.view else {
            return false;
        };
        crate::plugin::tab_bar::update_mouse_left_click(tab_bar, row, self.last_cols)
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, config: BTreeMap<String, String>) {
        self.view = ViewState::from(View::from(&config));
        let home_dir = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        match &mut self.view {
            ViewState::TabBar(tab_bar) => crate::plugin::tab_bar::load(tab_bar, home_dir),
            ViewState::Picker(picker) => crate::plugin::picker::load(picker, home_dir, &config),
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
        match &mut self.view {
            ViewState::TabBar(tab_bar) => {
                crate::plugin::tab_bar::render(tab_bar, rows, cols, &mut self.render_buf);
            }
            ViewState::Picker(picker) => {
                crate::plugin::picker::render(picker, rows, cols, &mut self.render_buf);
            }
        }
        if !self.render_buf.is_empty() {
            print!("{}", self.render_buf);
        }
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        match &mut self.view {
            ViewState::TabBar(tab_bar) => crate::plugin::tab_bar::pipe(tab_bar, &pipe_message),
            ViewState::Picker(picker) => crate::plugin::picker::pipe(picker, &pipe_message),
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

    use crate::plugin::main::State;
    use crate::plugin::main::View;
    use crate::plugin::main::ViewState;
    use crate::plugin::tab_bar::AGG_SYNC_PIPE;

    #[test]
    fn test_view_from_defaults_to_tab_bar_for_missing_or_unknown_view() {
        assert_eq!(View::from(&BTreeMap::new()), View::TabBar);
        assert_eq!(
            View::from(&BTreeMap::from([(String::from("view"), String::from("unknown"))])),
            View::TabBar
        );
    }

    #[test]
    fn test_view_from_selects_picker_for_picker_view() {
        assert_eq!(
            View::from(&BTreeMap::from([(String::from("view"), String::from("picker"))])),
            View::Picker
        );
    }

    #[test]
    fn test_pipe_returns_false_for_picker_view() {
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
            view: ViewState::Picker(Box::default()),
            ..Default::default()
        };

        assert2::assert!(!state.pipe(msg));
    }
}
