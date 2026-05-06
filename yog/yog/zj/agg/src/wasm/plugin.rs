use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use agg::Cmd;
use agg::GitStat;
use agg::ParseError;
use agg::TabIndicator;
use agg::TabStateEntry;
use ytil_agents::agent::AgentIcon;
use zellij_tile::prelude::*;

use crate::wasm::events::PipeEvent;
use crate::wasm::events::PipeEventError;
use crate::wasm::events::StateEvent;
use crate::wasm::state::State;
use crate::wasm::state::current_tab::CurrentTab;
use crate::wasm::state::current_tab::FocusedPane;
use crate::wasm::state::nudge::Nudge;
use crate::wasm::ui;

pub const SYNC_PIPE: &str = "agg-sync";

const CONTEXT_KEY_GIT_STAT: &str = "git-stat";

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug)]
pub struct StateSnapshotPayload {
    pub tab_id: usize,
    pub seq: u64,
    pub cwd: Option<PathBuf>,
    pub cmd: Cmd,
    pub indicator: TabIndicator,
    pub git_stat: GitStat,
}

impl StateSnapshotPayload {
    fn from_current_tab(value: &CurrentTab) -> Self {
        Self {
            tab_id: value.tab_id,
            seq: value.seq,
            cwd: value.cwd.clone(),
            cmd: value.display_cmd(),
            indicator: value.tab_indicator(),
            git_stat: value.git_stat,
        }
    }
}

impl TryFrom<&PipeMessage> for StateSnapshotPayload {
    type Error = PipeEventError;

    fn try_from(value: &PipeMessage) -> Result<Self, Self::Error> {
        let tab_id = value
            .args
            .get("tab_id")
            .ok_or(PipeEventError::Parse(ParseError::Missing("tab_id")))
            .and_then(|v| {
                v.parse::<usize>().map_err(|_| {
                    PipeEventError::Parse(ParseError::Invalid {
                        field: "tab_id",
                        value: v.clone(),
                    })
                })
            })?;
        let seq = value
            .args
            .get("seq")
            .ok_or(PipeEventError::Parse(ParseError::Missing("seq")))
            .and_then(|v| {
                v.parse::<u64>().map_err(|_| {
                    PipeEventError::Parse(ParseError::Invalid {
                        field: "seq",
                        value: v.clone(),
                    })
                })
            })?;
        let payload = value
            .payload
            .as_ref()
            .ok_or(PipeEventError::Parse(ParseError::Missing("state_snapshot payload")))?;
        let entry = TabStateEntry::try_from((tab_id, payload.as_str())).map_err(|e| {
            PipeEventError::Parse(ParseError::Invalid {
                field: "state_snapshot payload",
                value: e.to_string(),
            })
        })?;

        Ok(Self {
            tab_id,
            seq,
            cwd: entry.cwd,
            cmd: entry.cmd,
            indicator: entry.indicator,
            git_stat: entry.git_stat,
        })
    }
}

impl From<&StateSnapshotPayload> for MessageToPlugin {
    fn from(value: &StateSnapshotPayload) -> Self {
        let entry = TabStateEntry {
            tab_id: value.tab_id,
            cwd: value.cwd.clone(),
            cmd: value.cmd.clone(),
            indicator: value.indicator,
            git_stat: value.git_stat,
        };
        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "state_snapshot".to_string());
        args.insert("tab_id".to_string(), value.tab_id.to_string());
        args.insert("seq".to_string(), value.seq.to_string());
        Self::new(SYNC_PIPE.to_string())
            .with_args(args)
            .with_payload(entry.to_string())
    }
}

impl State {
    fn apply_and_handle_events(&mut self, events: &[StateEvent]) -> bool {
        let frame_changed = self.apply_all(events);
        handle_events(self, events);
        frame_changed || !events.is_empty()
    }

    fn update_permission_granted(&mut self) -> bool {
        update_permission_granted(self)
    }

    fn update_tabs(&mut self, mut tabs: Vec<TabInfo>) -> bool {
        let active_tab_id = tabs.iter().find(|tab| tab.active).map(|tab| tab.tab_id);
        let landing_focus = active_tab_id.and_then(|active_tab_id| {
            resolve_active_tab_landing_focus(active_tab_id, &tabs, self.current_tab.as_ref())
        });
        let events = self.events_from_tab_update(&mut tabs, landing_focus);
        let frame_changed = self.apply_all(&events);
        if let Some(active_tab_id) = active_tab_id {
            send_active_tab(active_tab_id);
        }
        handle_events(self, &events);
        frame_changed || !events.is_empty()
    }

    fn update_panes(&mut self, manifest: &PaneManifest) -> bool {
        let events = self.events_from_pane_update(manifest, |pane_id| get_pane_cwd(PaneId::Terminal(pane_id)).ok());
        self.apply_and_handle_events(&events)
    }

    fn update_pane_closed(&mut self, pane_id: u32) -> bool {
        let events = self.events_from_pane_closed(pane_id);
        self.apply_and_handle_events(&events)
    }

    fn update_cwd(&mut self, pane_id: u32, cwd: PathBuf) -> bool {
        let events = self.events_from_cwd_changed(pane_id, cwd);
        self.apply_and_handle_events(&events)
    }

    fn update_run_command_result(
        &mut self,
        exit_code: Option<i32>,
        stdout: &[u8],
        context: &BTreeMap<String, String>,
    ) -> bool {
        let Some(requested_cwd) = context.get(CONTEXT_KEY_GIT_STAT).map(PathBuf::from) else {
            return false;
        };
        let events = self.events_from_run_command_result(&requested_cwd, exit_code, stdout);
        self.apply_and_handle_events(&events)
    }

    fn update_mouse_left_click(&self, row: isize) -> bool {
        let Ok(row) = usize::try_from(row) else {
            return false;
        };
        let content_w = self.last_cols.saturating_sub(1);
        if let Some(tab_idx) = ui::tab_index_at_row(&self.frame, row, content_w)
            && let Some(tab) = self.all_tabs.get(tab_idx)
            && let Ok(pos) = u32::try_from(tab.position)
        {
            switch_tab_to(pos.saturating_add(1));
        }
        false
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        self.plugin_id = get_plugin_ids().plugin_id;
        self.home_dir = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
            PermissionType::MessageAndLaunchOtherPlugins,
        ]);
        subscribe(&[EventType::PermissionRequestResult]);
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
        ui::render_frame(&self.frame, rows, cols, &mut self.render_buf);
        if !self.render_buf.is_empty() {
            print!("{}", self.render_buf);
        }
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        let event = match PipeEvent::try_from(&pipe_message) {
            Ok(event) => event,
            Err(PipeEventError::UnknownMsgName(_) | PipeEventError::Parse(ParseError::Missing("source"))) => {
                return false;
            }
            Err(err) => {
                eprintln!("agg: {err}");
                return false;
            }
        };

        match event {
            PipeEvent::SyncRequest { requester_plugin_id } => {
                if requester_plugin_id == self.plugin_id {
                    return false;
                }
                send_current_tab_snapshot(self.current_tab.as_ref(), Some(requester_plugin_id));
                false
            }
            PipeEvent::ActiveTab { active_tab_id } => {
                let landing_focus =
                    resolve_active_tab_landing_focus(active_tab_id, &self.all_tabs, self.current_tab.as_ref());
                let events = self.events_from_active_tab(active_tab_id, landing_focus);
                let frame_changed = self.apply_all(&events);
                handle_events(self, &events);
                frame_changed || !events.is_empty()
            }
            PipeEvent::StateSnapshot {
                source_plugin_id,
                snapshot,
            } => {
                let events = self.events_from_state_snapshot(source_plugin_id, &snapshot);
                let frame_changed = self.apply_all(&events);
                frame_changed || !events.is_empty()
            }
            PipeEvent::Agent(agent_event) => {
                let events = self.events_from_agent_event(&agent_event);
                let frame_changed = self.apply_all(&events);
                handle_events(self, &events);
                frame_changed || !events.is_empty()
            }
        }
    }
}

fn update_permission_granted(state: &mut State) -> bool {
    subscribe(&[
        EventType::TabUpdate,
        EventType::PaneUpdate,
        EventType::PaneClosed,
        EventType::CwdChanged,
        EventType::Mouse,
        EventType::RunCommandResult,
    ]);
    set_selectable(false);
    state.sync_frame()
}

// No-op symbol for tests builds so unit tests can link/run in CI.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
const extern "C" fn host_run_plugin_command() {}

fn handle_events(state: &mut State, events: &[StateEvent]) {
    for event in events {
        match event {
            StateEvent::AgentIdle { .. } | StateEvent::FocusChanged { .. } | StateEvent::CwdChanged { .. } => {
                run_current_tab_git_stat(state.current_tab.as_ref());
                send_current_tab_snapshot(state.current_tab.as_ref(), None);
            }
            StateEvent::TabCreated { .. }
            | StateEvent::TabRemapped { .. }
            | StateEvent::GitStatChanged { .. }
            | StateEvent::AgentDetected { .. }
            | StateEvent::AgentBusy { .. }
            | StateEvent::AgentLost { .. }
            | StateEvent::ActiveTabChanged { .. }
            | StateEvent::BecameActive => send_current_tab_snapshot(state.current_tab.as_ref(), None),
            StateEvent::SyncRequested => send_sync_request(),
            StateEvent::PanesChanged { .. }
            | StateEvent::RemoteTabUpdated { .. }
            | StateEvent::TopologyChanged
            | StateEvent::AllTabsReplaced { .. } => {}
        }
    }
    for (pane_id, nudge) in state.nudges() {
        state.mark_nudged(pane_id);
        send_nudge(&state.home_dir, &nudge);
    }
}

fn send_nudge(home_dir: &Path, nudge: &Nudge) {
    let summary = nudge.summary();
    let body = nudge.body();
    let icon_path = AgentIcon::from(nudge.agent).path(home_dir);
    let icon_path = icon_path.to_string_lossy();
    let tab_id = nudge.tab_id.to_string();
    let pane_id = nudge.pane_id.to_string();
    let args = [
        "agg",
        "nudge",
        summary.as_str(),
        body.as_str(),
        tab_id.as_str(),
        pane_id.as_str(),
        icon_path.as_ref(),
    ];
    run_command(&args, BTreeMap::new());
}

fn send_sync_request() {
    let mut args = BTreeMap::new();
    args.insert("type".to_string(), "sync_request".to_string());
    pipe_message_to_plugin(MessageToPlugin::new(SYNC_PIPE.to_string()).with_args(args));
}

fn send_active_tab(active_tab_id: usize) {
    let mut args = BTreeMap::new();
    args.insert("type".to_string(), "active_tab".to_string());
    args.insert("tab_id".to_string(), active_tab_id.to_string());
    pipe_message_to_plugin(MessageToPlugin::new(SYNC_PIPE.to_string()).with_args(args));
}

fn resolve_active_tab_landing_focus(
    active_tab_id: usize,
    tabs: &[TabInfo],
    current_tab: Option<&CurrentTab>,
) -> Option<FocusedPane> {
    resolve_active_tab_landing_focus_with(
        active_tab_id,
        tabs,
        current_tab,
        || get_focused_pane_info().ok(),
        get_pane_info,
    )
}

fn resolve_active_tab_landing_focus_with<GetFocusedPaneInfo, GetPaneInfo>(
    active_tab_id: usize,
    tabs: &[TabInfo],
    current_tab: Option<&CurrentTab>,
    mut get_focused_pane_info: GetFocusedPaneInfo,
    mut get_pane_info: GetPaneInfo,
) -> Option<FocusedPane>
where
    GetFocusedPaneInfo: FnMut() -> Option<(usize, PaneId)>,
    GetPaneInfo: FnMut(PaneId) -> Option<PaneInfo>,
{
    let active_tab_position = tabs.iter().find(|tab| tab.tab_id == active_tab_id)?.position;
    if let Some((focused_tab_position, focused_pane_id)) = get_focused_pane_info()
        && focused_tab_position == active_tab_position
        && let Some(pane) = get_pane_info(focused_pane_id)
        && let Some(focused_pane) = crate::wasm::state::pane::focused_pane_from_pane_info(&pane)
    {
        return Some(focused_pane);
    }

    let current_tab = current_tab?;
    current_tab.pane_ids.iter().find_map(|pane_id| {
        let pane = get_pane_info(PaneId::Terminal(*pane_id))?;
        if !pane.is_focused {
            return None;
        }
        crate::wasm::state::pane::focused_pane_from_pane_info(&pane)
    })
}

fn send_current_tab_snapshot(current_tab: Option<&CurrentTab>, target_plugin_id: Option<u32>) {
    let Some(current_tab) = current_tab else {
        return;
    };
    let mut message = MessageToPlugin::from(&StateSnapshotPayload::from_current_tab(current_tab));
    if let Some(target_plugin_id) = target_plugin_id {
        message = message.with_destination_plugin_id(target_plugin_id);
    }
    pipe_message_to_plugin(message);
}

fn run_current_tab_git_stat(current_tab: Option<&CurrentTab>) {
    let Some(current_tab) = current_tab else {
        return;
    };
    let Some(ref cwd) = current_tab.cwd else {
        return;
    };
    let cwd_str = cwd.display().to_string();
    let mut context = BTreeMap::new();
    context.insert(CONTEXT_KEY_GIT_STAT.into(), cwd_str.clone());
    let args: Vec<&str> = vec!["agg", "git-stat", &cwd_str];
    run_command_with_env_variables_and_cwd(&args, BTreeMap::new(), cwd.clone(), context);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::state::current_tab::FocusedPaneLabel;

    #[test]
    fn test_sync_request_is_ignored_when_not_sent_by_plugin() {
        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "sync_request".to_string());
        let msg = PipeMessage {
            source: PipeSource::Cli("x".to_string()),
            name: SYNC_PIPE.to_string(),
            payload: None,
            args,
            is_private: false,
        };
        let parsed = PipeEvent::try_from(&msg);
        assert2::assert!(let Err(PipeEventError::Parse(ParseError::Missing("source"))) = parsed);
    }

    #[test]
    fn test_resolve_active_tab_landing_focus_falls_back_to_focused_tracked_pane_when_host_focus_missing() {
        let tabs = vec![TabInfo {
            tab_id: 20,
            position: 1,
            active: true,
            ..Default::default()
        }];
        let current_tab = CurrentTab {
            pane_ids: std::iter::once(42).collect(),
            ..CurrentTab::new(20)
        };

        let landing_focus = resolve_active_tab_landing_focus_with(
            20,
            &tabs,
            Some(&current_tab),
            || None,
            |pane_id| match pane_id {
                PaneId::Terminal(42) => Some(PaneInfo {
                    id: 42,
                    is_focused: true,
                    terminal_command: Some("claude".to_string()),
                    ..Default::default()
                }),
                PaneId::Terminal(_) | PaneId::Plugin(_) => None,
            },
        );

        assert_eq!(
            landing_focus,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            })
        );
    }

    #[test]
    fn test_resolve_active_tab_landing_focus_prefers_matching_host_focus_over_tracked_scan() {
        let tabs = vec![TabInfo {
            tab_id: 20,
            position: 1,
            active: true,
            ..Default::default()
        }];
        let current_tab = CurrentTab {
            pane_ids: std::iter::once(42).collect(),
            ..CurrentTab::new(20)
        };

        let landing_focus = resolve_active_tab_landing_focus_with(
            20,
            &tabs,
            Some(&current_tab),
            || Some((1, PaneId::Terminal(99))),
            |pane_id| match pane_id {
                PaneId::Terminal(99) => Some(PaneInfo {
                    id: 99,
                    is_focused: true,
                    title: "Cursor Agent".to_string(),
                    ..Default::default()
                }),
                PaneId::Terminal(42) => Some(PaneInfo {
                    id: 42,
                    is_focused: true,
                    terminal_command: Some("claude".to_string()),
                    ..Default::default()
                }),
                PaneId::Terminal(_) | PaneId::Plugin(_) => None,
            },
        );

        assert_eq!(
            landing_focus,
            Some(FocusedPane {
                id: 99,
                label: Some(FocusedPaneLabel::Title("Cursor …".to_string())),
            })
        );
    }
}
