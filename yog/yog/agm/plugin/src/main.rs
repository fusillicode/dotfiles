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
use agm_core::TabStateEntry;
use ui::TabRow;
use zellij_tile::prelude::*;

mod ui;

const CONTEXT_KEY_GIT_STAT: &str = "git-stat";
const SYNC_PIPE: &str = "agm-sync";

struct CurrentTab {
    tab_id: usize,
    seq: u64,
    pane_ids: HashSet<u32>,
    focused_pane_id: Option<u32>,
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
            focused_pane_id: None,
            cwd: None,
            agent_by_pane: HashMap::new(),
            git_stat: GitStat::default(),
        }
    }
}

#[derive(Debug)]
struct PipeEventParseError(String);

impl PipeEventParseError {
    fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl std::fmt::Display for PipeEventParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PipeEventParseError {}

struct SyncRequestPayload {
    requester_tab_id: usize,
}

impl SyncRequestPayload {
    fn parse(msg: &PipeMessage) -> Result<Self, PipeEventParseError> {
        let requester_tab_id = msg
            .args
            .get("requester_tab_id")
            .ok_or_else(|| PipeEventParseError::new("missing requester_tab_id"))?
            .parse::<usize>()
            .map_err(|_| PipeEventParseError::new("invalid requester_tab_id"))?;
        Ok(Self { requester_tab_id })
    }

    fn to_message(&self) -> MessageToPlugin {
        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "sync_request".to_string());
        args.insert("requester_tab_id".to_string(), self.requester_tab_id.to_string());
        MessageToPlugin::new(SYNC_PIPE.to_string()).with_args(args)
    }
}

#[derive(Clone)]
struct StateSnapshotPayload {
    tab_id: usize,
    target_tab_id: Option<usize>,
    seq: u64,
    cwd: Option<PathBuf>,
    cmd: Cmd,
    git_stat: GitStat,
}

impl StateSnapshotPayload {
    fn with_target(mut self, target_tab_id: usize) -> Self {
        self.target_tab_id = Some(target_tab_id);
        self
    }

    fn parse(msg: &PipeMessage) -> Result<Self, PipeEventParseError> {
        let tab_id = msg
            .args
            .get("tab_id")
            .ok_or_else(|| PipeEventParseError::new("missing tab_id"))?
            .parse::<usize>()
            .map_err(|_| PipeEventParseError::new("invalid tab_id"))?;
        let seq = msg
            .args
            .get("seq")
            .ok_or_else(|| PipeEventParseError::new("missing seq"))?
            .parse::<u64>()
            .map_err(|_| PipeEventParseError::new("invalid seq"))?;
        let target_tab_id = msg
            .args
            .get("target_tab_id")
            .map(|v| {
                v.parse::<usize>()
                    .map_err(|_| PipeEventParseError::new("invalid target_tab_id"))
            })
            .transpose()?;
        let payload = msg
            .payload
            .as_ref()
            .ok_or_else(|| PipeEventParseError::new("missing state_snapshot payload"))?;
        let entry = TabStateEntry::parse_file_content(tab_id, payload)
            .map_err(|e| PipeEventParseError::new(format!("invalid state_snapshot payload: {e}")))?;

        Ok(Self {
            tab_id,
            target_tab_id,
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
        if let Some(target_tab_id) = self.target_tab_id {
            args.insert("target_tab_id".to_string(), target_tab_id.to_string());
        }

        MessageToPlugin::new(SYNC_PIPE.to_string())
            .with_args(args)
            .with_payload(entry.to_file_content())
    }
}

impl From<&CurrentTab> for StateSnapshotPayload {
    fn from(value: &CurrentTab) -> Self {
        Self {
            tab_id: value.tab_id,
            target_tab_id: None,
            seq: value.seq,
            cwd: value.cwd.clone(),
            cmd: local_display_cmd(value),
            git_stat: value.git_stat,
        }
    }
}

enum PipeEvent {
    SyncRequest(SyncRequestPayload),
    StateSnapshot(StateSnapshotPayload),
    Agent(AgentEventPayload),
}

impl PipeEvent {
    fn parse(msg: &PipeMessage) -> Result<Option<Self>, PipeEventParseError> {
        match msg.name.as_str() {
            SYNC_PIPE => match msg.args.get("type").map(String::as_str) {
                Some("sync_request") => Ok(Some(Self::SyncRequest(SyncRequestPayload::parse(msg)?))),
                Some("state_snapshot") => Ok(Some(Self::StateSnapshot(StateSnapshotPayload::parse(msg)?))),
                Some(other) => Err(PipeEventParseError::new(format!("unknown sync message type {other:?}"))),
                None => Err(PipeEventParseError::new("missing sync message type")),
            },
            AGENTS_PIPE => {
                let pane_id = msg
                    .args
                    .get("pane_id")
                    .ok_or_else(|| PipeEventParseError::new("missing pane_id"))?;
                let agent = msg
                    .args
                    .get("agent")
                    .ok_or_else(|| PipeEventParseError::new("missing agent"))?;
                let payload = msg.payload.as_deref().unwrap_or("");
                let payload = AgentEventPayload::parse(pane_id, agent, payload)
                    .map_err(|e| PipeEventParseError::new(e.to_string()))?;
                Ok(Some(Self::Agent(payload)))
            }
            _ => Ok(None),
        }
    }
}

#[derive(Default)]
struct State {
    plugin_id: u32,
    all_tabs: Vec<TabInfo>,
    current_tab: Option<CurrentTab>,
    other_tabs: HashMap<usize, StateSnapshotPayload>,
    sync_requested: bool,
    home_dir: PathBuf,
    frame: Vec<TabRow>,
    last_cols: usize,
    render_buf: String,
}

register_plugin!(State);

impl State {
    fn local_tab_id(&self) -> Option<usize> {
        self.current_tab.as_ref().map(|local| local.tab_id)
    }

    fn tab_position(&self, tab_id: usize) -> Option<usize> {
        self.all_tabs
            .iter()
            .find(|tab| tab.tab_id == tab_id)
            .map(|tab| tab.position)
    }

    fn discover_local_tab_id(&self, manifest: &PaneManifest) -> Option<usize> {
        for (tab_pos, panes) in &manifest.panes {
            let owns_this_tab = panes.iter().any(|pane| pane.is_plugin && pane.id == self.plugin_id);
            if !owns_this_tab {
                continue;
            }

            if let Some(tab) = self.all_tabs.iter().find(|tab| tab.position == *tab_pos) {
                return Some(tab.tab_id);
            }
        }

        None
    }

    fn ensure_local_tab(&mut self, manifest: &PaneManifest) -> bool {
        let Some(tab_id) = self.discover_local_tab_id(manifest) else {
            return false;
        };

        if self.current_tab.as_ref().is_some_and(|local| local.tab_id == tab_id) {
            return false;
        }

        self.current_tab = Some(CurrentTab::new(tab_id));
        self.other_tabs.remove(&tab_id);
        self.sync_requested = false;
        true
    }

    fn refresh_local_from_manifest(&mut self, manifest: &PaneManifest) -> (bool, bool, bool) {
        let Some(local_tab_id) = self.local_tab_id() else {
            return (false, false, false);
        };
        let Some(tab_pos) = self.tab_position(local_tab_id) else {
            return (false, false, false);
        };
        let Some(panes) = manifest.panes.get(&tab_pos) else {
            return (false, false, false);
        };

        let mut pane_ids = HashSet::new();
        let mut focused_pane_id = None;
        for pane in panes.iter().filter(|pane| !pane.is_plugin) {
            pane_ids.insert(pane.id);
            if pane.is_focused {
                focused_pane_id = Some(pane.id);
            }
        }

        let Some(local) = self.current_tab.as_mut() else {
            return (false, false, false);
        };

        let prev_cmd = local_display_cmd(local);

        if local.pane_ids != pane_ids {
            local.pane_ids = pane_ids.clone();
            local.agent_by_pane.retain(|pane_id, _| pane_ids.contains(pane_id));
        }

        let mut focused_changed = false;
        if local.focused_pane_id != focused_pane_id {
            local.focused_pane_id = focused_pane_id;
            focused_changed = true;
        }

        let mut cwd_changed = false;
        if let Some(pane_id) = local.focused_pane_id
            && (focused_changed || local.cwd.is_none())
            && let Ok(cwd) = get_pane_cwd(PaneId::Terminal(pane_id))
            && local.cwd.as_ref() != Some(&cwd)
        {
            local.cwd = Some(cwd);
            cwd_changed = true;
        }

        let cmd_changed = prev_cmd != local_display_cmd(local);
        (focused_changed, cwd_changed, cmd_changed)
    }

    fn update_local_cwd(&mut self, pane_id: u32, cwd: PathBuf) -> bool {
        let Some(local) = self.current_tab.as_mut() else {
            return false;
        };
        if local.focused_pane_id != Some(pane_id) {
            return false;
        }
        if local.cwd.as_ref() == Some(&cwd) {
            return false;
        }

        local.cwd = Some(cwd);
        true
    }

    fn update_local_agent_event(&mut self, event_payload: AgentEventPayload) -> (bool, bool) {
        let Some(local) = self.current_tab.as_mut() else {
            return (false, false);
        };
        if !local.pane_ids.contains(&event_payload.pane_id) {
            return (false, false);
        }

        let prev_cmd = local_display_cmd(local);
        if !apply_agent_event(local, &event_payload) {
            return (false, false);
        }

        let cmd_changed = prev_cmd != local_display_cmd(local);
        let should_refresh_git = matches!(event_payload.kind, AgentEventKind::Idle);
        (cmd_changed, should_refresh_git)
    }

    fn update_local_git_stat(&mut self, exit_code: Option<i32>, stdout: &[u8]) -> bool {
        if exit_code != Some(0) {
            return false;
        }

        let Some(cwd) = self.current_tab.as_ref().and_then(|local| local.cwd.clone()) else {
            return false;
        };

        let output = String::from_utf8_lossy(stdout);
        for line in output.lines() {
            let Ok((path, git_stat)) = GitStat::parse_line(line).inspect_err(|e| eprintln!("agm: {e}")) else {
                continue;
            };

            if path != cwd {
                continue;
            }

            let Some(local) = self.current_tab.as_mut() else {
                return false;
            };
            if local.git_stat == git_stat {
                return false;
            }

            local.git_stat = git_stat;
            return true;
        }

        false
    }

    fn bump_local_seq(&mut self) {
        if let Some(local) = self.current_tab.as_mut() {
            local.seq = local.seq.saturating_add(1);
        }
    }

    fn send_sync_request(&mut self) {
        let Some(requester_tab_id) = self.local_tab_id() else {
            return;
        };

        pipe_message_to_plugin(SyncRequestPayload { requester_tab_id }.to_message());
        self.sync_requested = true;
    }

    fn send_local_snapshot(&self, target_tab_id: Option<usize>) {
        let Some(local) = self.current_tab.as_ref() else {
            return;
        };

        let snapshot = StateSnapshotPayload::from(local);
        let snapshot = if let Some(target_tab_id) = target_tab_id {
            snapshot.with_target(target_tab_id)
        } else {
            snapshot
        };
        pipe_message_to_plugin(snapshot.to_message());
    }

    fn run_local_git_stat(&self) {
        let Some(cwd) = self.current_tab.as_ref().and_then(|local| local.cwd.clone()) else {
            return;
        };

        let cwd_str = cwd.display().to_string();
        let args: Vec<&str> = vec!["agm", "git-stat", &cwd_str];
        let mut context = BTreeMap::new();
        context.insert(CONTEXT_KEY_GIT_STAT.into(), String::new());
        run_command_with_env_variables_and_cwd(&args, BTreeMap::new(), cwd, context);
    }

    fn apply_remote_snapshot(&mut self, mut snapshot: StateSnapshotPayload) -> bool {
        if self.local_tab_id() == Some(snapshot.tab_id) {
            return false;
        }

        if let Some(target_tab_id) = snapshot.target_tab_id
            && self.local_tab_id() != Some(target_tab_id)
        {
            return false;
        }

        if !self.all_tabs.iter().any(|tab| tab.tab_id == snapshot.tab_id) {
            return false;
        }

        if self
            .other_tabs
            .get(&snapshot.tab_id)
            .is_some_and(|remote| snapshot.seq <= remote.seq)
        {
            return false;
        }

        let changed = self.other_tabs.get(&snapshot.tab_id).is_none_or(|remote| {
            remote.cwd != snapshot.cwd || remote.cmd != snapshot.cmd || remote.git_stat != snapshot.git_stat
        });

        snapshot.target_tab_id = None;
        self.other_tabs.insert(snapshot.tab_id, snapshot);
        changed
    }

    fn prune_state_for_closed_tabs(&mut self) {
        let known_tabs: HashSet<usize> = self.all_tabs.iter().map(|tab| tab.tab_id).collect();
        if let Some(local_tab_id) = self.local_tab_id()
            && !known_tabs.contains(&local_tab_id)
        {
            self.current_tab = None;
            self.sync_requested = false;
        }

        let local_tab_id = self.local_tab_id();
        self.other_tabs
            .retain(|tab_id, _| known_tabs.contains(tab_id) && Some(*tab_id) != local_tab_id);
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
            PipeEvent::SyncRequest(request) => {
                if self.local_tab_id() == Some(request.requester_tab_id) {
                    return false;
                }
                self.send_local_snapshot(Some(request.requester_tab_id));
                false
            }
            PipeEvent::StateSnapshot(snapshot) => {
                if !self.apply_remote_snapshot(snapshot) {
                    return false;
                }
                self.sync_frame()
            }
            PipeEvent::Agent(agent_event) => {
                let (cmd_changed, should_refresh_git) = self.update_local_agent_event(agent_event);
                if cmd_changed {
                    self.bump_local_seq();
                    self.send_local_snapshot(None);
                }

                if should_refresh_git {
                    self.run_local_git_stat();
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

            Event::TabUpdate(tabs) => {
                self.all_tabs = tabs;
                self.prune_state_for_closed_tabs();
                self.sync_frame()
            }

            Event::PaneUpdate(manifest) => {
                let local_created = self.ensure_local_tab(&manifest);
                let (focused_changed, cwd_changed, cmd_changed) = self.refresh_local_from_manifest(&manifest);

                if self.current_tab.is_some() && !self.sync_requested {
                    self.send_sync_request();
                }

                if focused_changed || cwd_changed {
                    self.run_local_git_stat();
                }

                if cwd_changed || cmd_changed {
                    self.bump_local_seq();
                    self.send_local_snapshot(None);
                }

                if !(local_created || cwd_changed || cmd_changed) {
                    return false;
                }
                self.sync_frame()
            }

            Event::CwdChanged(PaneId::Terminal(pane_id), cwd, _clients) => {
                if !self.update_local_cwd(pane_id, cwd) {
                    return false;
                }

                self.bump_local_seq();
                self.send_local_snapshot(None);
                self.run_local_git_stat();
                self.sync_frame()
            }

            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                if !context.contains_key(CONTEXT_KEY_GIT_STAT) {
                    return false;
                }
                if !self.update_local_git_stat(exit_code, &stdout) {
                    return false;
                }

                self.bump_local_seq();
                self.send_local_snapshot(None);
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
        let event = match PipeEvent::parse(&pipe_message) {
            Ok(Some(event)) => event,
            Ok(None) => return false,
            Err(err) => {
                eprintln!("agm: {err}");
                return false;
            }
        };

        self.handle_pipe_event(event)
    }
}

fn local_display_cmd(local: &CurrentTab) -> Cmd {
    local
        .agent_by_pane
        .values()
        .filter_map(|cmd| match cmd {
            Cmd::BusyAgent(agent) => Some((agent.priority(), 1_u8, cmd.clone())),
            Cmd::IdleAgent(agent) => Some((agent.priority(), 0_u8, cmd.clone())),
            _ => None,
        })
        .max_by_key(|(priority, busy, _)| (*priority, *busy))
        .map(|(_, _, cmd)| cmd)
        .unwrap_or(Cmd::None)
}

fn apply_agent_event(local: &mut CurrentTab, event_payload: &AgentEventPayload) -> bool {
    let mut current = local
        .agent_by_pane
        .get(&event_payload.pane_id)
        .cloned()
        .unwrap_or(Cmd::None);
    if !apply_agent_event_to_cmd(&mut current, event_payload) {
        return false;
    }

    if current.is_agent() {
        local.agent_by_pane.insert(event_payload.pane_id, current);
    } else {
        local.agent_by_pane.remove(&event_payload.pane_id);
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
            if state.local_tab_id() == Some(tab.tab_id)
                && let Some(local) = state.current_tab.as_ref()
            {
                return TabRow::new(
                    tab,
                    local.cwd.as_ref(),
                    local_display_cmd(local),
                    local.git_stat,
                    &state.home_dir,
                );
            }

            if let Some(remote) = state.other_tabs.get(&tab.tab_id) {
                return TabRow::new(
                    tab,
                    remote.cwd.as_ref(),
                    remote.cmd.clone(),
                    remote.git_stat,
                    &state.home_dir,
                );
            }

            TabRow::new(tab, None, Cmd::None, GitStat::default(), &state.home_dir)
        })
        .collect()
}
