use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use agm_core::AGENTS_PIPE;
use agm_core::Agent;
use agm_core::AgentEventKind;
use agm_core::AgentEventPayload;
use agm_core::Cmd;
use agm_core::GitStat;
use agm_core::ParseError;
use agm_core::TabStateEntry;
use ui::TabRow;
use zellij_tile::prelude::*;

mod ui;

const CONTEXT_KEY_GIT_STAT: &str = "git-stat";
const SYNC_PIPE: &str = "agm-sync";

// no-op symbol for tests builds so unit tests can link/run in CI.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
extern "C" fn host_run_plugin_command() {}

#[derive(Clone, PartialEq)]
#[cfg_attr(test, derive(Debug))]
struct FocusedPane {
    id: u32,
    cmd: Option<String>,
}

#[cfg_attr(test, derive(Debug, PartialEq))]
struct CurrentTab {
    tab_id: usize,
    seq: u64,
    pane_ids: HashSet<u32>,
    focused_pane: Option<FocusedPane>,
    cwd: Option<PathBuf>,
    agent_by_pane: HashMap<u32, Cmd>,
    git_stat: GitStat,
}

impl CurrentTab {
    fn new(tab_id: usize) -> Self {
        Self {
            tab_id,
            seq: 0,
            pane_ids: HashSet::new(),
            focused_pane: None,
            cwd: None,
            agent_by_pane: HashMap::new(),
            git_stat: GitStat::default(),
        }
    }

    fn cmd(&self) -> Cmd {
        self.agent_by_pane
            .values()
            .filter_map(|cmd| match cmd {
                Cmd::BusyAgent(agent) => Some((agent.priority(), 1_u8, cmd.clone())),
                Cmd::IdleAgent(agent) => Some((agent.priority(), 0_u8, cmd.clone())),
                _ => None,
            })
            .max_by_key(|(priority, busy, _)| (*priority, *busy))
            .map(|(_, _, cmd)| cmd)
            .or_else(|| {
                let focused_pane = self.focused_pane.as_ref()?;
                let cmd_line = focused_pane.cmd.as_deref()?;
                if let Some(exe) = parse_running_command(cmd_line)
                    && let Some(agent) = Agent::detect(&exe)
                {
                    return Some(Cmd::IdleAgent(agent));
                }
                Some(Cmd::Running(cmd_line.to_string()))
            })
            .unwrap_or(Cmd::None)
    }
}

#[derive(Debug)]
enum PipeEventError {
    Parse(ParseError),
    UnknownMsgName(String),
}

impl std::fmt::Display for PipeEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipeEventError::Parse(err) => write!(f, "{err}"),
            PipeEventError::UnknownMsgName(name) => write!(f, "unknown message name {name:?}"),
        }
    }
}

impl std::error::Error for PipeEventError {}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Clone, Debug)]
struct StateSnapshotPayload {
    tab_id: usize,
    seq: u64,
    cwd: Option<PathBuf>,
    cmd: Cmd,
    git_stat: GitStat,
}

impl StateSnapshotPayload {
    fn parse(msg: &PipeMessage) -> Result<Self, PipeEventError> {
        let tab_id = msg
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
        let seq = msg
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
        let payload = msg
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

    fn to_message(&self) -> MessageToPlugin {
        let entry = TabStateEntry {
            tab_id: self.tab_id,
            cwd: self.cwd.clone(),
            cmd: self.cmd.clone(),
            git_stat: self.git_stat,
        };

        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "state_snapshot".to_string());
        args.insert("tab_id".to_string(), self.tab_id.to_string());
        args.insert("seq".to_string(), self.seq.to_string());

        MessageToPlugin::new(SYNC_PIPE.to_string())
            .with_args(args)
            .with_payload(entry.to_string())
    }
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

enum PipeEvent {
    SyncRequest {
        requester_plugin_id: u32,
    },
    StateSnapshot {
        source_plugin_id: u32,
        snapshot: StateSnapshotPayload,
    },
    Agent(AgentEventPayload),
}

impl TryFrom<&PipeMessage> for PipeEvent {
    type Error = PipeEventError;

    fn try_from(msg: &PipeMessage) -> Result<Self, Self::Error> {
        match msg.name.as_str() {
            SYNC_PIPE => match msg.args.get("type").map(String::as_str) {
                Some("sync_request") => {
                    let requester_plugin_id =
                        Self::source_plugin_id(msg).ok_or(PipeEventError::Parse(ParseError::Missing("source")))?;
                    Ok(Self::SyncRequest { requester_plugin_id })
                }
                Some("state_snapshot") => {
                    let source_plugin_id =
                        Self::source_plugin_id(msg).ok_or(PipeEventError::Parse(ParseError::Missing("source")))?;
                    let snapshot = StateSnapshotPayload::parse(msg)?;
                    Ok(Self::StateSnapshot {
                        source_plugin_id,
                        snapshot,
                    })
                }
                Some(other) => Err(PipeEventError::UnknownMsgName(other.to_string())),
                None => Err(PipeEventError::Parse(ParseError::Missing("sync message type"))),
            },
            AGENTS_PIPE => {
                let pane_id = msg
                    .args
                    .get("pane_id")
                    .ok_or(PipeEventError::Parse(ParseError::Missing("pane_id")))?;
                let agent = msg
                    .args
                    .get("agent")
                    .ok_or(PipeEventError::Parse(ParseError::Missing("agent")))?;
                let payload = msg.payload.as_deref().unwrap_or("");
                let payload = AgentEventPayload::parse(pane_id, agent, payload).map_err(|e| {
                    PipeEventError::Parse(ParseError::Invalid {
                        field: "agent",
                        value: e.to_string(),
                    })
                })?;
                Ok(Self::Agent(payload))
            }
            _ => Err(PipeEventError::UnknownMsgName(msg.name.clone())),
        }
    }
}

impl PipeEvent {
    fn source_plugin_id(msg: &PipeMessage) -> Option<u32> {
        match msg.source {
            PipeSource::Plugin(plugin_id) => Some(plugin_id),
            _ => None,
        }
    }
}

#[derive(Default)]
struct State {
    plugin_id: u32,
    all_tabs: Vec<TabInfo>,
    current_tab: Option<CurrentTab>,
    other_tabs: HashMap<u32, StateSnapshotPayload>,
    sync_requested: bool,
    home_dir: PathBuf,
    frame: Vec<TabRow>,
    last_cols: usize,
    render_buf: String,
}

register_plugin!(State);

fn zellij_terminal_pane_cwd(pane_id: u32) -> Option<PathBuf> {
    get_pane_cwd(PaneId::Terminal(pane_id)).ok()
}

impl State {
    fn current_tab_id(&self) -> Option<usize> {
        self.current_tab.as_ref().map(|t| t.tab_id)
    }

    fn current_tab_position_in_manifest(&self, manifest: &PaneManifest) -> Option<usize> {
        manifest.panes.iter().find_map(|(tab_pos, panes)| {
            panes
                .iter()
                .any(|pane| pane.is_plugin && pane.id == self.plugin_id)
                .then_some(*tab_pos)
        })
    }

    fn discover_current_tab_id(&self, manifest: &PaneManifest) -> Option<usize> {
        let tab_pos = self.current_tab_position_in_manifest(manifest)?;
        self.all_tabs
            .iter()
            .find(|tab| tab.position == tab_pos)
            .map(|tab| tab.tab_id)
    }

    fn ensure_current_tab(&mut self, manifest: &PaneManifest) -> bool {
        if let Some(current_tab_id) = self.current_tab.as_ref().map(|current_tab| current_tab.tab_id) {
            if self.all_tabs.iter().any(|tab| tab.tab_id == current_tab_id) {
                return false;
            }

            let Some(tab_id) = self.discover_current_tab_id(manifest) else {
                return false;
            };
            if current_tab_id == tab_id {
                return false;
            }

            if let Some(current_tab) = self.current_tab.as_mut() {
                current_tab.tab_id = tab_id;
            }
            self.sync_requested = false;
            return true;
        }

        let Some(tab_id) = self.discover_current_tab_id(manifest) else {
            return false;
        };

        self.current_tab = Some(CurrentTab::new(tab_id));
        self.sync_requested = false;
        true
    }

    fn refresh_current_tab_from_manifest(
        &mut self,
        manifest: &PaneManifest,
        mut resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
    ) -> (bool, bool, bool) {
        if self.current_tab.is_none() {
            return (false, false, false);
        }
        let Some(tab_pos) = self.current_tab_position_in_manifest(manifest) else {
            return (false, false, false);
        };
        let Some(panes) = manifest.panes.get(&tab_pos) else {
            return (false, false, false);
        };

        let mut pane_ids = HashSet::new();
        let mut focused_pane = None;
        for pane in panes
            .iter()
            .filter(|pane| !pane.is_plugin && !pane.exited && !pane.is_held)
        {
            pane_ids.insert(pane.id);
            if pane.is_focused {
                focused_pane = Some(FocusedPane {
                    id: pane.id,
                    cmd: focused_pane_running_command(pane),
                });
            }
        }

        let Some(current_tab) = self.current_tab.as_mut() else {
            return (false, false, false);
        };

        let prev_cmd = current_tab.cmd();

        if current_tab.pane_ids != pane_ids {
            current_tab.pane_ids = pane_ids.clone();
            current_tab
                .agent_by_pane
                .retain(|pane_id, _| pane_ids.contains(pane_id));
        }

        let mut focused_changed = false;
        if current_tab.focused_pane != focused_pane {
            current_tab.focused_pane = focused_pane;
            focused_changed = true;
        }

        reconcile_agent_by_pane_with_manifest(current_tab, panes);

        let mut cwd_changed = false;
        if let Some(focused_pane) = current_tab.focused_pane.as_ref()
            && (focused_changed || current_tab.cwd.is_none())
            && let Some(cwd) = resolve_pane_cwd(focused_pane.id)
            && current_tab.cwd.as_ref() != Some(&cwd)
        {
            current_tab.cwd = Some(cwd);
            cwd_changed = true;
        }

        let cmd_changed = prev_cmd != current_tab.cmd();
        (focused_changed, cwd_changed, cmd_changed)
    }

    fn update_current_tab_cwd(&mut self, pane_id: u32, cwd: PathBuf) -> bool {
        let Some(current_tab) = self.current_tab.as_mut() else {
            return false;
        };
        if current_tab.focused_pane.as_ref().map(|fp| fp.id) != Some(pane_id) {
            return false;
        }
        if current_tab.cwd.as_ref() == Some(&cwd) {
            return false;
        }

        current_tab.cwd = Some(cwd);
        true
    }

    fn update_current_tab_agent_event(&mut self, event_payload: AgentEventPayload) -> (bool, bool) {
        let Some(current_tab) = self.current_tab.as_mut() else {
            return (false, false);
        };
        if !current_tab.pane_ids.contains(&event_payload.pane_id) {
            return (false, false);
        }

        let prev_cmd = current_tab.cmd();
        if !apply_agent_event(current_tab, &event_payload) {
            return (false, false);
        }

        let cmd_changed = prev_cmd != current_tab.cmd();
        let should_refresh_git = matches!(event_payload.kind, AgentEventKind::Idle);
        (cmd_changed, should_refresh_git)
    }

    fn update_current_tab_git_stat(&mut self, requested_cwd: &PathBuf, exit_code: Option<i32>, stdout: &[u8]) -> bool {
        if exit_code != Some(0) {
            return false;
        }

        let Some(current_tab_cwd) = self
            .current_tab
            .as_ref()
            .and_then(|current_tab| current_tab.cwd.clone())
        else {
            return false;
        };
        if &current_tab_cwd != requested_cwd {
            return false;
        }

        let output = String::from_utf8_lossy(stdout);
        let mut new_stat = None;
        for line in output.lines() {
            let Ok((path, git_stat)) = GitStat::parse_line(line).inspect_err(|e| eprintln!("agm: {e}")) else {
                continue;
            };

            if path != *requested_cwd {
                continue;
            }
            new_stat = Some(git_stat);
            break;
        }

        let Some(current_tab) = self.current_tab.as_mut() else {
            return false;
        };
        let new_stat = new_stat.unwrap_or_default();
        if current_tab.git_stat == new_stat {
            return false;
        }

        current_tab.git_stat = new_stat;
        true
    }

    fn bump_current_tab_seq(&mut self) {
        if let Some(current_tab) = self.current_tab.as_mut() {
            current_tab.seq = current_tab.seq.saturating_add(1);
        }
    }

    fn send_sync_request(&mut self) {
        if self.current_tab.is_none() {
            return;
        }

        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "sync_request".to_string());
        pipe_message_to_plugin(MessageToPlugin::new(SYNC_PIPE.to_string()).with_args(args));
        self.sync_requested = true;
    }

    fn remap_current_tab_id_after_tab_update(&mut self, prev_tabs: &[TabInfo]) -> bool {
        let Some(current_tab) = self.current_tab.as_mut() else {
            return false;
        };
        if self.all_tabs.iter().any(|tab| tab.tab_id == current_tab.tab_id) {
            return false;
        }

        let prev_ids: HashSet<usize> = prev_tabs.iter().map(|tab| tab.tab_id).collect();
        let next_ids: HashSet<usize> = self.all_tabs.iter().map(|tab| tab.tab_id).collect();
        let removed: HashSet<usize> = prev_ids.difference(&next_ids).copied().collect();
        if !removed.contains(&current_tab.tab_id) {
            return false;
        }

        let mut added: Vec<&TabInfo> = self
            .all_tabs
            .iter()
            .filter(|tab| !prev_ids.contains(&tab.tab_id))
            .collect();
        if added.is_empty() {
            return false;
        }

        if added.len() > 1
            && let Some(prev_current_tab) = prev_tabs.iter().find(|tab| tab.tab_id == current_tab.tab_id)
        {
            let by_name: Vec<&TabInfo> = added
                .iter()
                .copied()
                .filter(|tab| tab.name == prev_current_tab.name)
                .collect();
            if by_name.len() == 1 {
                added = by_name;
            }
        }

        if added.len() != 1 {
            return false;
        }

        current_tab.tab_id = added[0].tab_id;
        self.sync_requested = false;
        true
    }

    fn apply_remote_snapshot(&mut self, source_plugin_id: u32, snapshot: StateSnapshotPayload) -> bool {
        if source_plugin_id == self.plugin_id {
            return false;
        }

        if self.current_tab_id() == Some(snapshot.tab_id) {
            return false;
        }

        if !self.all_tabs.iter().any(|tab| tab.tab_id == snapshot.tab_id) {
            return false;
        }

        if self
            .other_tabs
            .get(&source_plugin_id)
            .is_some_and(|remote| snapshot.seq <= remote.seq)
        {
            return false;
        }

        let changed = self.other_tabs.get(&source_plugin_id).is_none_or(|remote| {
            remote.cwd != snapshot.cwd || remote.cmd != snapshot.cmd || remote.git_stat != snapshot.git_stat
        });

        self.other_tabs.retain(|other_source_plugin_id, remote| {
            *other_source_plugin_id == source_plugin_id || remote.tab_id != snapshot.tab_id
        });
        self.other_tabs.insert(source_plugin_id, snapshot);
        changed
    }

    fn prune_state_for_closed_tabs(&mut self) {
        let known_tabs: HashSet<usize> = self.all_tabs.iter().map(|tab| tab.tab_id).collect();
        self.other_tabs.retain(|_, remote| known_tabs.contains(&remote.tab_id));
    }

    fn remote_snapshot_for_tab(&self, tab_id: usize) -> Option<&StateSnapshotPayload> {
        self.other_tabs
            .values()
            .filter(|remote| remote.tab_id == tab_id)
            .max_by_key(|remote| remote.seq)
    }

    fn sync_frame(&mut self) -> bool {
        let next = compute_frame(self);
        if self.frame == next {
            return false;
        }
        self.frame = next;
        true
    }

    fn handle_pipe_event(&mut self, event: PipeEvent) -> bool {
        match event {
            PipeEvent::SyncRequest { requester_plugin_id } => {
                if requester_plugin_id == self.plugin_id {
                    return false;
                }
                send_current_tab_snapshot(self.current_tab.as_ref(), Some(requester_plugin_id));
                false
            }
            PipeEvent::StateSnapshot {
                source_plugin_id,
                snapshot,
            } => {
                if !self.apply_remote_snapshot(source_plugin_id, snapshot) {
                    return false;
                }
                self.sync_frame()
            }
            PipeEvent::Agent(agent_event) => {
                let (cmd_changed, should_refresh_git) = self.update_current_tab_agent_event(agent_event);
                if cmd_changed {
                    self.bump_current_tab_seq();
                    send_current_tab_snapshot(self.current_tab.as_ref(), None);
                }

                if should_refresh_git {
                    run_current_tab_git_stat(self.current_tab.as_ref());
                }

                if !cmd_changed {
                    return false;
                }
                self.sync_frame()
            }
        }
    }
}

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
                tabs.sort_by_key(|tab| tab.position);
                let prev_tabs = std::mem::replace(&mut self.all_tabs, tabs);
                let was_active = self
                    .current_tab_id()
                    .is_some_and(|tab_id| tab_is_active(&prev_tabs, tab_id));
                let topology_changed = topology_changed(&prev_tabs, &self.all_tabs);
                let remapped_current_tab_id = self.remap_current_tab_id_after_tab_update(&prev_tabs);
                let is_active = self
                    .current_tab_id()
                    .is_some_and(|tab_id| tab_is_active(&self.all_tabs, tab_id));

                if topology_changed {
                    self.sync_requested = false;
                }

                if remapped_current_tab_id {
                    self.bump_current_tab_seq();
                    send_current_tab_snapshot(self.current_tab.as_ref(), None);
                }

                self.prune_state_for_closed_tabs();

                if self.current_tab.is_some() && !self.sync_requested {
                    self.send_sync_request();
                }

                if !was_active && is_active {
                    run_current_tab_git_stat(self.current_tab.as_ref());
                }

                self.sync_frame()
            }

            Event::PaneUpdate(manifest) => {
                let current_tab_created = self.ensure_current_tab(&manifest);
                let (focused_changed, cwd_changed, cmd_changed) =
                    self.refresh_current_tab_from_manifest(&manifest, zellij_terminal_pane_cwd);

                if self.current_tab.is_some() && !self.sync_requested {
                    self.send_sync_request();
                }

                if focused_changed || cwd_changed {
                    run_current_tab_git_stat(self.current_tab.as_ref());
                }

                if current_tab_created || cwd_changed || cmd_changed {
                    self.bump_current_tab_seq();
                    send_current_tab_snapshot(self.current_tab.as_ref(), None);
                }

                if !(current_tab_created || cwd_changed || cmd_changed) {
                    return false;
                }
                self.sync_frame()
            }

            Event::CwdChanged(PaneId::Terminal(pane_id), cwd, _clients) => {
                if !self.update_current_tab_cwd(pane_id, cwd) {
                    return false;
                }

                self.bump_current_tab_seq();
                send_current_tab_snapshot(self.current_tab.as_ref(), None);
                run_current_tab_git_stat(self.current_tab.as_ref());
                self.sync_frame()
            }

            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                let Some(requested_cwd) = context.get(CONTEXT_KEY_GIT_STAT).map(PathBuf::from) else {
                    return false;
                };
                if !self.update_current_tab_git_stat(&requested_cwd, exit_code, &stdout) {
                    return false;
                }

                self.bump_current_tab_seq();
                send_current_tab_snapshot(self.current_tab.as_ref(), None);
                self.sync_frame()
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

        self.handle_pipe_event(event)
    }
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

fn send_current_tab_snapshot(current_tab: Option<&CurrentTab>, target_plugin_id: Option<u32>) {
    let Some(current_tab) = current_tab else {
        return;
    };
    let mut message = StateSnapshotPayload::from(current_tab).to_message();
    if let Some(target_plugin_id) = target_plugin_id {
        message = message.with_destination_plugin_id(target_plugin_id);
    }
    pipe_message_to_plugin(message);
}

fn tab_is_active(tabs: &[TabInfo], tab_id: usize) -> bool {
    tabs.iter().any(|tab| tab.active && tab.tab_id == tab_id)
}

fn topology_changed(x: &[TabInfo], y: &[TabInfo]) -> bool {
    if x.len() != y.len() {
        return true;
    }
    x.iter()
        .zip(y.iter())
        .any(|(a, b)| a.tab_id != b.tab_id || a.position != b.position)
}

fn focused_pane_running_command(pane: &PaneInfo) -> Option<String> {
    if pane.exited || pane.is_held {
        return None;
    }

    pane.terminal_command
        .as_deref()
        .and_then(parse_running_command)
        .or_else(|| parse_pane_title(&pane.title))
}

/// Drop hook-driven agent state when the pane no longer runs that agent (e.g. Codex quit → shell).
///
/// Codex `hooks.json` has no session-exit hook; `Stop` is end-of-turn, not "quit the TUI". The
/// manifest still updates the terminal command/title, so we reconcile here on every pane refresh.
fn reconcile_agent_by_pane_with_manifest(current_tab: &mut CurrentTab, panes: &[PaneInfo]) {
    let tracked: Vec<u32> = current_tab.agent_by_pane.keys().copied().collect();
    for pane_id in tracked {
        let Some(cmd) = current_tab.agent_by_pane.get(&pane_id) else {
            continue;
        };
        if cmd.agent_name().is_none() {
            continue;
        }
        let Some(stored) = cmd.agent_name().and_then(|n| Agent::from_name(n).ok()) else {
            continue;
        };

        let Some(pane) = panes.iter().find(|p| p.id == pane_id && !p.is_plugin) else {
            current_tab.agent_by_pane.remove(&pane_id);
            continue;
        };
        if pane.exited || pane.is_held {
            current_tab.agent_by_pane.remove(&pane_id);
            continue;
        }

        let detected = focused_pane_running_command(pane).and_then(|exe| Agent::detect(&exe));
        let has_exec_line = pane.terminal_command.as_ref().is_some_and(|s| !s.trim().is_empty());

        match detected {
            Some(d) if d != stored => {}
            // Title-only metadata is ambiguous (e.g. cwd path while Codex is running); rely on the
            // shell-reported command line to detect "agent is gone" after quit.
            None if has_exec_line => {
                current_tab.agent_by_pane.remove(&pane_id);
            }
            _ => {}
        }
    }
}

fn parse_running_command(command: &str) -> Option<String> {
    let executable = command.split_whitespace().next()?;
    let executable = executable.rsplit('/').next().unwrap_or(executable);
    if executable.is_empty() || matches!(executable, "zsh" | "bash" | "fish") {
        return None;
    }
    Some(executable.to_string())
}

fn parse_pane_title(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.starts_with('~') || trimmed.starts_with('/') {
        return None;
    }
    parse_running_command(trimmed)
}

fn apply_agent_event(current_tab: &mut CurrentTab, event_payload: &AgentEventPayload) -> bool {
    let mut current = current_tab
        .agent_by_pane
        .get(&event_payload.pane_id)
        .cloned()
        .unwrap_or(Cmd::None);
    if !apply_agent_event_to_cmd(&mut current, event_payload) {
        return false;
    }

    if current.agent_name().is_some() {
        current_tab.agent_by_pane.insert(event_payload.pane_id, current);
    } else {
        current_tab.agent_by_pane.remove(&event_payload.pane_id);
    }
    true
}

fn apply_agent_event_to_cmd(cmd: &mut Cmd, event_payload: &AgentEventPayload) -> bool {
    let current_agent = cmd.agent_name().and_then(|name| Agent::from_name(name).ok());
    if let Some(current_agent) = current_agent
        && event_payload.agent.priority() < current_agent.priority()
    {
        return false;
    }

    let next = Cmd::from(event_payload);
    if *cmd == next {
        return false;
    }
    *cmd = next;
    true
}

fn compute_frame(state: &State) -> Vec<TabRow> {
    state
        .all_tabs
        .iter()
        .map(|tab| {
            if state.current_tab_id() == Some(tab.tab_id)
                && let Some(current_tab) = state.current_tab.as_ref()
            {
                return TabRow::new(
                    tab,
                    current_tab.cwd.as_ref(),
                    current_tab.cmd(),
                    current_tab.git_stat,
                    state.home_dir.as_path(),
                );
            }

            if let Some(remote) = state.remote_snapshot_for_tab(tab.tab_id) {
                return TabRow::new(
                    tab,
                    remote.cwd.as_ref(),
                    remote.cmd.clone(),
                    remote.git_stat,
                    state.home_dir.as_path(),
                );
            }

            TabRow::new(tab, None, Cmd::None, GitStat::default(), state.home_dir.as_path())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn noop_pane_cwd(_pane_id: u32) -> Option<PathBuf> {
        None
    }

    fn tab_with_name(tab_id: usize, position: usize, name: &str) -> TabInfo {
        TabInfo {
            tab_id,
            position,
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn plugin_pane(id: u32) -> PaneInfo {
        PaneInfo {
            id,
            is_plugin: true,
            ..Default::default()
        }
    }

    fn terminal_pane(id: u32, is_focused: bool) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            ..Default::default()
        }
    }

    fn terminal_pane_with_command(id: u32, is_focused: bool, command: &str) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            terminal_command: Some(command.to_string()),
            ..Default::default()
        }
    }

    fn terminal_pane_with_title(id: u32, is_focused: bool, title: &str) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            title: title.to_string(),
            ..Default::default()
        }
    }

    fn manifest(entries: Vec<(usize, Vec<PaneInfo>)>) -> PaneManifest {
        let panes = entries.into_iter().collect::<HashMap<usize, Vec<PaneInfo>>>();
        PaneManifest { panes }
    }

    fn snapshot(tab_id: usize, seq: u64, cmd: Cmd) -> StateSnapshotPayload {
        StateSnapshotPayload {
            tab_id,
            seq,
            cwd: None,
            cmd,
            git_stat: GitStat::default(),
        }
    }

    #[test]
    fn pane_update_before_tab_update_does_not_rebind_current_tab_id() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a"), tab_with_name(11, 1, "b")],
            ..Default::default()
        };

        let initial_manifest = manifest(vec![(0, vec![plugin_pane(7), terminal_pane(42, true)])]);
        assert!(state.ensure_current_tab(&initial_manifest));

        let current_tab = state.current_tab.as_mut().expect("missing current tab");
        current_tab.pane_ids.insert(42);
        current_tab.focused_pane = Some(FocusedPane { id: 42, cmd: None });
        current_tab.cwd = Some(PathBuf::from("/tmp/project"));
        current_tab.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        // Simulate a tab move where PaneUpdate arrives before TabUpdate:
        // manifest already reflects new position, while all_tabs is still stale.
        let moved_manifest = manifest(vec![(1, vec![plugin_pane(7), terminal_pane(42, true)])]);
        assert!(!state.ensure_current_tab(&moved_manifest));
        let _ = state.refresh_current_tab_from_manifest(&moved_manifest, noop_pane_cwd);

        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        let expected_current_tab = CurrentTab {
            tab_id: 10,
            seq: 0,
            pane_ids: [42].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 42, cmd: None }),
            cwd: Some(PathBuf::from("/tmp/project")),
            agent_by_pane: [(42, Cmd::BusyAgent(Agent::Codex))].into_iter().collect(),
            git_stat: GitStat::default(),
        };
        pretty_assertions::assert_eq!(current_tab, &expected_current_tab);
    }

    #[test]
    fn pane_update_rebinds_current_tab_id_only_after_old_id_disappears() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(11, 0, "b"), tab_with_name(99, 1, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let current_tab = state.current_tab.as_mut().expect("missing current tab");
        current_tab.pane_ids.insert(42);
        current_tab.focused_pane = Some(FocusedPane { id: 42, cmd: None });
        current_tab.cwd = Some(PathBuf::from("/tmp/project"));
        current_tab.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        let manifest = manifest(vec![(1, vec![plugin_pane(7), terminal_pane(42, true)])]);
        assert!(state.ensure_current_tab(&manifest));
        let _ = state.refresh_current_tab_from_manifest(&manifest, noop_pane_cwd);

        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        let expected_current_tab = CurrentTab {
            tab_id: 99,
            seq: 0,
            pane_ids: [42].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 42, cmd: None }),
            cwd: Some(PathBuf::from("/tmp/project")),
            agent_by_pane: [(42, Cmd::BusyAgent(Agent::Codex))].into_iter().collect(),
            git_stat: GitStat::default(),
        };
        pretty_assertions::assert_eq!(current_tab, &expected_current_tab);
    }

    #[test]
    fn remap_current_tab_id_after_tab_update_can_use_tab_name_when_many_ids_changed() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(10)),
            all_tabs: vec![tab_with_name(30, 1, "agent"), tab_with_name(40, 0, "shell")],
            ..Default::default()
        };

        let prev_tabs = vec![tab_with_name(10, 0, "agent"), tab_with_name(20, 1, "shell")];
        assert!(state.remap_current_tab_id_after_tab_update(&prev_tabs));
        pretty_assertions::assert_eq!(state.current_tab_id(), Some(30));
    }

    #[test]
    fn remote_snapshot_sequence_is_tracked_per_source_plugin() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(1)),
            all_tabs: vec![tab_with_name(1, 0, "local"), tab_with_name(2, 1, "remote")],
            ..Default::default()
        };

        assert!(state.apply_remote_snapshot(100, snapshot(2, 10, Cmd::BusyAgent(Agent::Codex))));
        assert!(state.apply_remote_snapshot(200, snapshot(2, 1, Cmd::IdleAgent(Agent::Claude))));

        let expected = HashMap::from([(200, snapshot(2, 1, Cmd::IdleAgent(Agent::Claude)))]);
        pretty_assertions::assert_eq!(state.other_tabs, expected);
    }

    #[test]
    fn current_tab_cmd_falls_back_to_focused_non_agent_command() {
        let mut current_tab = CurrentTab::new(1);
        current_tab.focused_pane = Some(FocusedPane {
            id: 0,
            cmd: Some("cargo".to_string()),
        });
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::Running("cargo".to_string()));
    }

    #[test]
    fn current_tab_cmd_prioritizes_agents_over_focused_non_agent_command() {
        let mut current_tab = CurrentTab::new(1);
        current_tab.focused_pane = Some(FocusedPane {
            id: 0,
            cmd: Some("cargo".to_string()),
        });
        current_tab.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::BusyAgent(Agent::Codex));
    }

    #[test]
    fn current_tab_cmd_shows_idle_agent_when_focus_executable_matches_agent() {
        let mut current_tab = CurrentTab::new(1);
        current_tab.focused_pane = Some(FocusedPane {
            id: 0,
            cmd: Some("/opt/homebrew/bin/codex".to_string()),
        });
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::IdleAgent(Agent::Codex));
    }

    #[test]
    fn refresh_current_tab_from_manifest_picks_focused_command_name() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let current_tab = state.current_tab.as_mut().expect("missing current tab");
        current_tab.pane_ids.insert(42);
        current_tab.focused_pane = Some(FocusedPane { id: 42, cmd: None });
        current_tab.cwd = Some(PathBuf::from("/tmp/project"));

        let pane = terminal_pane_with_command(42, true, "/usr/bin/cargo test -p agm-plugin");
        let manifest = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let (_focused_changed, _cwd_changed, cmd_changed) =
            state.refresh_current_tab_from_manifest(&manifest, noop_pane_cwd);

        assert!(cmd_changed);
        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        pretty_assertions::assert_eq!(
            current_tab.focused_pane.as_ref().and_then(|fp| fp.cmd.as_deref()),
            Some("cargo")
        );
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::Running("cargo".to_string()));
    }

    #[test]
    fn refresh_current_tab_from_manifest_falls_back_to_focused_pane_title() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let current_tab = state.current_tab.as_mut().expect("missing current tab");
        current_tab.pane_ids.insert(42);
        current_tab.focused_pane = Some(FocusedPane { id: 42, cmd: None });
        current_tab.cwd = Some(PathBuf::from("/tmp/project"));

        let pane = terminal_pane_with_title(42, true, "nvim");
        let manifest = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let (_focused_changed, _cwd_changed, cmd_changed) =
            state.refresh_current_tab_from_manifest(&manifest, noop_pane_cwd);

        assert!(cmd_changed);
        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        pretty_assertions::assert_eq!(
            current_tab.focused_pane.as_ref().and_then(|fp| fp.cmd.as_deref()),
            Some("nvim")
        );
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::Running("nvim".to_string()));
    }

    #[test]
    fn refresh_current_tab_from_manifest_ignores_shell_or_path_titles() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let current_tab = state.current_tab.as_mut().expect("missing current tab");
        current_tab.pane_ids.insert(42);
        current_tab.focused_pane = Some(FocusedPane {
            id: 42,
            cmd: Some("nvim".to_string()),
        });
        current_tab.cwd = Some(PathBuf::from("/tmp/project"));

        let pane = terminal_pane_with_title(42, true, "/tmp/project");
        let manifest = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let (_focused_changed, _cwd_changed, cmd_changed) =
            state.refresh_current_tab_from_manifest(&manifest, noop_pane_cwd);

        assert!(cmd_changed);
        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        pretty_assertions::assert_eq!(current_tab.focused_pane.as_ref().and_then(|fp| fp.cmd.as_deref()), None);
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::None);
    }

    #[test]
    fn refresh_current_tab_from_manifest_clears_agent_state_when_pane_process_changes() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let current_tab = state.current_tab.as_mut().expect("missing current tab");
        current_tab.pane_ids.insert(42);
        current_tab.focused_pane = Some(FocusedPane {
            id: 42,
            cmd: Some("codex".to_string()),
        });
        current_tab.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        let pane = terminal_pane_with_command(42, true, "/bin/zsh");
        let manifest = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let _ = state.refresh_current_tab_from_manifest(&manifest, noop_pane_cwd);

        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        pretty_assertions::assert_eq!(current_tab.agent_by_pane.get(&42), None);
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::None);
    }

    #[test]
    fn parse_pane_title_ignores_shell_names() {
        pretty_assertions::assert_eq!(parse_pane_title("zsh"), None);
        pretty_assertions::assert_eq!(parse_pane_title("bash"), None);
        pretty_assertions::assert_eq!(parse_pane_title("fish"), None);
        pretty_assertions::assert_eq!(parse_pane_title("cargo test"), Some("cargo".to_string()));
    }

    #[test]
    fn sync_request_is_ignored_when_not_sent_by_plugin() {
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
}
