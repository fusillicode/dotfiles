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

#[derive(Clone)]
struct StateSnapshotPayload {
    tab_id: usize,
    seq: u64,
    cwd: Option<PathBuf>,
    cmd: Cmd,
    git_stat: GitStat,
}

impl StateSnapshotPayload {
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
        let payload = msg
            .payload
            .as_ref()
            .ok_or_else(|| PipeEventParseError::new("missing state_snapshot payload"))?;
        let entry = TabStateEntry::parse_file_content(tab_id, payload)
            .map_err(|e| PipeEventParseError::new(format!("invalid state_snapshot payload: {e}")))?;

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
            .with_payload(entry.to_file_content())
    }
}

impl From<&CurrentTab> for StateSnapshotPayload {
    fn from(value: &CurrentTab) -> Self {
        Self {
            tab_id: value.tab_id,
            seq: value.seq,
            cwd: value.cwd.clone(),
            cmd: local_display_cmd(value),
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

impl PipeEvent {
    fn source_plugin_id(msg: &PipeMessage) -> Option<u32> {
        match msg.source {
            PipeSource::Plugin(plugin_id) => Some(plugin_id),
            _ => None,
        }
    }

    fn parse(msg: &PipeMessage) -> Result<Option<Self>, PipeEventParseError> {
        match msg.name.as_str() {
            SYNC_PIPE => match msg.args.get("type").map(String::as_str) {
                Some("sync_request") => {
                    let Some(requester_plugin_id) = Self::source_plugin_id(msg) else {
                        return Ok(None);
                    };
                    Ok(Some(Self::SyncRequest { requester_plugin_id }))
                }
                Some("state_snapshot") => {
                    let Some(source_plugin_id) = Self::source_plugin_id(msg) else {
                        return Ok(None);
                    };
                    let snapshot = StateSnapshotPayload::parse(msg)?;
                    Ok(Some(Self::StateSnapshot {
                        source_plugin_id,
                        snapshot,
                    }))
                }
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
    other_tabs: HashMap<u32, StateSnapshotPayload>,
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

    fn local_tab_position_in_manifest(&self, manifest: &PaneManifest) -> Option<usize> {
        manifest.panes.iter().find_map(|(tab_pos, panes)| {
            panes
                .iter()
                .any(|pane| pane.is_plugin && pane.id == self.plugin_id)
                .then_some(*tab_pos)
        })
    }

    fn discover_local_tab_id(&self, manifest: &PaneManifest) -> Option<usize> {
        let tab_pos = self.local_tab_position_in_manifest(manifest)?;
        self.all_tabs
            .iter()
            .find(|tab| tab.position == tab_pos)
            .map(|tab| tab.tab_id)
    }

    fn ensure_local_tab(&mut self, manifest: &PaneManifest) -> bool {
        if let Some(local_tab_id) = self.current_tab.as_ref().map(|local| local.tab_id) {
            if self.all_tabs.iter().any(|tab| tab.tab_id == local_tab_id) {
                return false;
            }

            let Some(tab_id) = self.discover_local_tab_id(manifest) else {
                return false;
            };
            if local_tab_id == tab_id {
                return false;
            }

            if let Some(local) = self.current_tab.as_mut() {
                local.tab_id = tab_id;
            }
            self.sync_requested = false;
            return true;
        }

        let Some(tab_id) = self.discover_local_tab_id(manifest) else {
            return false;
        };

        self.current_tab = Some(CurrentTab::new(tab_id));
        self.sync_requested = false;
        true
    }

    fn refresh_local_from_manifest(&mut self, manifest: &PaneManifest) -> (bool, bool, bool) {
        if self.current_tab.is_none() {
            return (false, false, false);
        }
        let Some(tab_pos) = self.local_tab_position_in_manifest(manifest) else {
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
        if self.current_tab.is_none() {
            return;
        }

        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "sync_request".to_string());
        pipe_message_to_plugin(MessageToPlugin::new(SYNC_PIPE.to_string()).with_args(args));
        self.sync_requested = true;
    }

    fn send_local_snapshot(&self, target_plugin_id: Option<u32>) {
        let Some(local) = self.current_tab.as_ref() else {
            return;
        };

        let mut message = StateSnapshotPayload::from(local).to_message();
        if let Some(target_plugin_id) = target_plugin_id {
            message = message.with_destination_plugin_id(target_plugin_id);
        }
        pipe_message_to_plugin(message);
    }

    fn topology_changed(prev_tabs: &[TabInfo], next_tabs: &[TabInfo]) -> bool {
        if prev_tabs.len() != next_tabs.len() {
            return true;
        }

        prev_tabs
            .iter()
            .zip(next_tabs.iter())
            .any(|(prev, next)| prev.tab_id != next.tab_id || prev.position != next.position)
    }

    fn remap_local_tab_id_after_tab_update(&mut self, prev_tabs: &[TabInfo]) -> bool {
        let Some(local) = self.current_tab.as_mut() else {
            return false;
        };
        if self.all_tabs.iter().any(|tab| tab.tab_id == local.tab_id) {
            return false;
        }

        let prev_ids: HashSet<usize> = prev_tabs.iter().map(|tab| tab.tab_id).collect();
        let next_ids: HashSet<usize> = self.all_tabs.iter().map(|tab| tab.tab_id).collect();
        let removed: HashSet<usize> = prev_ids.difference(&next_ids).copied().collect();
        if !removed.contains(&local.tab_id) {
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
            && let Some(prev_local_tab) = prev_tabs.iter().find(|tab| tab.tab_id == local.tab_id)
        {
            let by_name: Vec<&TabInfo> = added
                .iter()
                .copied()
                .filter(|tab| tab.name == prev_local_tab.name)
                .collect();
            if by_name.len() == 1 {
                added = by_name;
            }
        }

        if added.len() != 1 {
            return false;
        }

        local.tab_id = added[0].tab_id;
        self.sync_requested = false;
        true
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

    fn apply_remote_snapshot(&mut self, source_plugin_id: u32, snapshot: StateSnapshotPayload) -> bool {
        if source_plugin_id == self.plugin_id {
            return false;
        }

        if self.local_tab_id() == Some(snapshot.tab_id) {
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
                self.send_local_snapshot(Some(requester_plugin_id));
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

            Event::TabUpdate(mut tabs) => {
                let prev_tabs = self.all_tabs.clone();
                tabs.sort_by_key(|tab| tab.position);
                self.all_tabs = tabs;
                let topology_changed = Self::topology_changed(&prev_tabs, &self.all_tabs);
                let remapped_local_tab_id = self.remap_local_tab_id_after_tab_update(&prev_tabs);

                if topology_changed {
                    self.sync_requested = false;
                }

                if remapped_local_tab_id {
                    self.bump_local_seq();
                    self.send_local_snapshot(None);
                }

                self.prune_state_for_closed_tabs();

                if self.current_tab.is_some() && !self.sync_requested {
                    self.send_sync_request();
                }

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

                if local_created || cwd_changed || cmd_changed {
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

            if let Some(remote) = state.remote_snapshot_for_tab(tab.tab_id) {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use agm_core::Agent;
    use pretty_assertions::assert_eq;

    use super::*;

    fn tab_with_name(tab_id: usize, position: usize, name: &str) -> TabInfo {
        TabInfo {
            tab_id,
            position,
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn plugin_pane(plugin_id: u32) -> PaneInfo {
        PaneInfo {
            id: plugin_id,
            is_plugin: true,
            ..Default::default()
        }
    }

    fn terminal_pane(pane_id: u32, focused: bool) -> PaneInfo {
        PaneInfo {
            id: pane_id,
            is_focused: focused,
            ..Default::default()
        }
    }

    fn manifest(entries: Vec<(usize, Vec<PaneInfo>)>) -> PaneManifest {
        let panes = entries.into_iter().collect::<HashMap<usize, Vec<PaneInfo>>>();
        PaneManifest {
            panes,
            ..Default::default()
        }
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
    fn pane_update_before_tab_update_does_not_rebind_local_tab_id() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a"), tab_with_name(11, 1, "b")],
            ..Default::default()
        };

        let initial_manifest = manifest(vec![(0, vec![plugin_pane(7), terminal_pane(42, true)])]);
        assert!(state.ensure_local_tab(&initial_manifest));

        let local = state.current_tab.as_mut().expect("missing local tab");
        local.pane_ids.insert(42);
        local.focused_pane_id = Some(42);
        local.cwd = Some(PathBuf::from("/tmp/project"));
        local.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        // Simulate a tab move where PaneUpdate arrives before TabUpdate:
        // manifest already reflects new position, while all_tabs is still stale.
        let moved_manifest = manifest(vec![(1, vec![plugin_pane(7), terminal_pane(42, true)])]);
        assert!(!state.ensure_local_tab(&moved_manifest));
        let _ = state.refresh_local_from_manifest(&moved_manifest);

        let local = state.current_tab.as_ref().expect("missing local tab");
        assert_eq!(local.tab_id, 10);
        assert_eq!(local.cwd.as_deref(), Some(std::path::Path::new("/tmp/project")));
        assert_eq!(local_display_cmd(local), Cmd::BusyAgent(Agent::Codex));
    }

    #[test]
    fn pane_update_rebinds_local_tab_id_only_after_old_id_disappears() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(11, 0, "b"), tab_with_name(99, 1, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let local = state.current_tab.as_mut().expect("missing local tab");
        local.pane_ids.insert(42);
        local.focused_pane_id = Some(42);
        local.cwd = Some(PathBuf::from("/tmp/project"));
        local.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        let manifest = manifest(vec![(1, vec![plugin_pane(7), terminal_pane(42, true)])]);
        assert!(state.ensure_local_tab(&manifest));
        let _ = state.refresh_local_from_manifest(&manifest);

        let local = state.current_tab.as_ref().expect("missing local tab");
        assert_eq!(local.tab_id, 99);
        assert_eq!(local_display_cmd(local), Cmd::BusyAgent(Agent::Codex));
    }

    #[test]
    fn remap_local_tab_id_after_tab_update_can_use_tab_name_when_many_ids_changed() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(10)),
            all_tabs: vec![tab_with_name(30, 1, "agent"), tab_with_name(40, 0, "shell")],
            ..Default::default()
        };

        let prev_tabs = vec![tab_with_name(10, 0, "agent"), tab_with_name(20, 1, "shell")];
        assert!(state.remap_local_tab_id_after_tab_update(&prev_tabs));
        assert_eq!(state.local_tab_id(), Some(30));
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

        assert_eq!(state.other_tabs.len(), 1);
        assert_eq!(
            state.remote_snapshot_for_tab(2).map(|remote| remote.cmd.clone()),
            Some(Cmd::IdleAgent(Agent::Claude))
        );
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
        let parsed = PipeEvent::parse(&msg).expect("parse failed");
        assert!(parsed.is_none());
    }
}
