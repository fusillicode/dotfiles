use std::collections::BTreeMap;
use std::path::PathBuf;

use zellij_tile::prelude::Event;
use zellij_tile::prelude::EventType;
use zellij_tile::prelude::Mouse;
use zellij_tile::prelude::PaneId;
use zellij_tile::prelude::PaneManifest;
use zellij_tile::prelude::PermissionStatus;
use zellij_tile::prelude::PermissionType;
use zellij_tile::prelude::PipeMessage;
use zellij_tile::prelude::TabInfo;
use zellij_tile::prelude::ZellijPlugin;

use crate::plugin::tab_bar::TabBarState;

#[derive(Default)]
pub struct State {
    pub tab_bar: Box<TabBarState>,
    pub last_cols: usize,
    pub render_buf: String,
}

impl State {
    fn update_permission_granted(&mut self) -> bool {
        crate::plugin::tab_bar::update_permission_granted(&mut self.tab_bar)
    }

    fn update_tabs(&mut self, tabs: Vec<TabInfo>) -> bool {
        crate::plugin::tab_bar::update_tabs(&mut self.tab_bar, tabs)
    }

    fn update_panes(&mut self, manifest: &PaneManifest) -> bool {
        crate::plugin::tab_bar::update_panes(&mut self.tab_bar, manifest)
    }

    fn update_pane_closed(&mut self, pane_id: u32) -> bool {
        crate::plugin::tab_bar::update_pane_closed(&mut self.tab_bar, pane_id)
    }

    fn update_cwd(&mut self, pane_id: u32, cwd: PathBuf) -> bool {
        crate::plugin::tab_bar::update_cwd(&mut self.tab_bar, pane_id, cwd)
    }

    fn update_run_command_result(
        &mut self,
        exit_code: Option<i32>,
        stdout: &[u8],
        context: &BTreeMap<String, String>,
    ) -> bool {
        crate::plugin::tab_bar::update_run_command_result(&mut self.tab_bar, exit_code, stdout, context)
    }

    fn update_mouse_left_click(&self, row: isize) -> bool {
        crate::plugin::tab_bar::update_mouse_left_click(&self.tab_bar, row, self.last_cols)
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        let home_dir = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        crate::plugin::tab_bar::load(&mut self.tab_bar, home_dir);
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
            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                self.update_run_command_result(exit_code, &stdout, &context)
            }
            Event::Mouse(Mouse::LeftClick(row, _col)) => self.update_mouse_left_click(row),
            Event::ModeUpdate(_)
            | Event::Key(_)
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
            | Event::CommandChanged(..)
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
        crate::plugin::tab_bar::render(&self.tab_bar, rows, cols, &mut self.render_buf);
        if !self.render_buf.is_empty() {
            print!("{}", self.render_buf);
        }
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        crate::plugin::tab_bar::pipe(&mut self.tab_bar, &pipe_message)
    }
}

// No-op symbol for tests builds so unit tests can link/run in CI.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
const extern "C" fn host_run_plugin_command() {}
