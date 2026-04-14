use std::collections::BTreeMap;
use std::path::PathBuf;

use agm_core::Cmd;
use agm_core::ParseError;
use agm_core::TabStateEntry;
use agm_core::git_stat::GitStat;
use zellij_tile::prelude::*;

use crate::events::PipeEvent;
use crate::events::PipeEventError;
use crate::events::StateEvent;
use crate::state::CurrentTab;
use crate::state::State;

mod events;
mod state;
mod ui;

const CONTEXT_KEY_GIT_STAT: &str = "git-stat";
const SYNC_PIPE: &str = "agm-sync";

// No-op symbol for tests builds so unit tests can link/run in CI.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
extern "C" fn host_run_plugin_command() {}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Clone, Debug)]
pub struct StateSnapshotPayload {
    pub tab_id: usize,
    pub seq: u64,
    pub cwd: Option<PathBuf>,
    pub cmd: Cmd,
    pub git_stat: GitStat,
}

impl From<&CurrentTab> for StateSnapshotPayload {
    fn from(value: &CurrentTab) -> Self {
        Self {
            tab_id: value.tab_id,
            seq: value.seq,
            cwd: value.cwd.clone(),
            cmd: value.cmd(),
            git_stat: value.git_stat,
        }
    }
}

impl TryFrom<&PipeMessage> for StateSnapshotPayload {
    type Error = PipeEventError;

    fn try_from(value: &PipeMessage) -> core::result::Result<Self, Self::Error> {
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
            git_stat: value.git_stat,
        };
        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "state_snapshot".to_string());
        args.insert("tab_id".to_string(), value.tab_id.to_string());
        args.insert("seq".to_string(), value.seq.to_string());
        MessageToPlugin::new(SYNC_PIPE.to_string())
            .with_args(args)
            .with_payload(entry.to_string())
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        self.plugin_id = get_plugin_ids().plugin_id;
        self.home_dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .expect("error getting HOME env var");
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
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                subscribe(&[
                    EventType::TabUpdate,
                    EventType::PaneUpdate,
                    EventType::CwdChanged,
                    EventType::Mouse,
                    EventType::RunCommandResult,
                ]);
                set_selectable(false);
                self.sync_frame()
            }

            Event::TabUpdate(mut tabs) => {
                let active_tab_id = active_tab_id_from_tabs(&tabs);
                let events = self.events_from_tab_update(&mut tabs);
                let frame_changed = self.apply_all(&events);
                if let Some(active_tab_id) = active_tab_id {
                    send_active_tab(active_tab_id);
                }
                handle_events(self.current_tab.as_ref(), &events);
                frame_changed || !events.is_empty()
            }

            Event::PaneUpdate(manifest) => {
                let events = self.events_from_pane_update(&manifest, zellij_terminal_pane_cwd);
                let frame_changed = self.apply_all(&events);
                handle_events(self.current_tab.as_ref(), &events);
                frame_changed || !events.is_empty()
            }

            Event::CwdChanged(PaneId::Terminal(pane_id), cwd, _clients) => {
                let events = self.events_from_cwd_changed(pane_id, cwd);
                let frame_changed = self.apply_all(&events);
                handle_events(self.current_tab.as_ref(), &events);
                frame_changed || !events.is_empty()
            }

            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                let Some(requested_cwd) = context.get(CONTEXT_KEY_GIT_STAT).map(PathBuf::from) else {
                    return false;
                };
                let events = self.events_from_run_command_result(&requested_cwd, exit_code, &stdout);
                let frame_changed = self.apply_all(&events);
                handle_events(self.current_tab.as_ref(), &events);
                frame_changed || !events.is_empty()
            }

            Event::Mouse(Mouse::LeftClick(row, _col)) => {
                let Ok(row_u) = usize::try_from(row) else {
                    return false;
                };
                let content_w = self.last_cols.saturating_sub(1);
                if let Some(tab_idx) = ui::tab_index_at_row(&self.frame, row_u, content_w)
                    && let Some(tab) = self.all_tabs.get(tab_idx)
                    && let Ok(pos) = u32::try_from(tab.position)
                {
                    switch_tab_to(pos + 1);
                }
                false
            }

            _ => false,
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
            Err(PipeEventError::UnknownMsgName(_)) | Err(PipeEventError::Parse(ParseError::Missing("source"))) => {
                return false;
            }
            Err(err) => {
                eprintln!("agm: {err}");
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
                let events = self.events_from_active_tab(active_tab_id);
                let frame_changed = self.apply_all(&events);
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
                handle_events(self.current_tab.as_ref(), &events);
                frame_changed || !events.is_empty()
            }
        }
    }
}

fn handle_events(current_tab: Option<&CurrentTab>, events: &[StateEvent]) {
    for event in events {
        match event {
            StateEvent::AgentIdle { .. } | StateEvent::FocusMoved { .. } | StateEvent::CwdChanged { .. } => {
                run_current_tab_git_stat(current_tab);
                send_current_tab_snapshot(current_tab, None);
            }
            StateEvent::TabCreated { .. }
            | StateEvent::TabRemapped { .. }
            | StateEvent::GitStatChanged { .. }
            | StateEvent::AgentDetected { .. }
            | StateEvent::AgentBusy { .. }
            | StateEvent::AgentLost { .. } => send_current_tab_snapshot(current_tab, None),
            StateEvent::SyncRequested => send_sync_request(),
            StateEvent::ActiveTabChanged { .. } => {}
            StateEvent::PanesChanged { .. }
            | StateEvent::RemoteTabUpdated { .. }
            | StateEvent::TopologyChanged
            | StateEvent::BecameActive
            | StateEvent::AllTabsReplaced { .. } => {}
        }
    }
}

fn zellij_terminal_pane_cwd(pane_id: u32) -> Option<PathBuf> {
    get_pane_cwd(PaneId::Terminal(pane_id)).ok()
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

fn active_tab_id_from_tabs(tabs: &[TabInfo]) -> Option<usize> {
    tabs.iter().find(|tab| tab.active).map(|tab| tab.tab_id)
}

fn send_current_tab_snapshot(current_tab: Option<&CurrentTab>, target_plugin_id: Option<u32>) {
    let Some(current_tab) = current_tab else {
        return;
    };
    let mut message = MessageToPlugin::from(&StateSnapshotPayload::from(current_tab));
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
    let args: Vec<&str> = vec!["agm", "git-stat", &cwd_str];
    let mut context = BTreeMap::new();
    context.insert(CONTEXT_KEY_GIT_STAT.into(), cwd_str.clone());
    run_command_with_env_variables_and_cwd(&args, BTreeMap::new(), cwd.to_path_buf(), context);
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_active_tab_id_from_tabs_returns_active_tab_even_without_current_tab_state() {
        let tabs = vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                active: false,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            },
        ];

        pretty_assertions::assert_eq!(active_tab_id_from_tabs(&tabs), Some(20));
    }
}
