use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use agm_core::Agent;
use agm_core::AgentEventKind;
use agm_core::AgentEventPayload;
use agm_core::Cmd;
use agm_core::GitStat;
use zellij_tile::prelude::*;

use crate::SYNC_PIPE;
use crate::StateSnapshotPayload;
use crate::ui::TabRow;

#[derive(Default)]
pub struct State {
    pub plugin_id: u32,
    pub all_tabs: Vec<TabInfo>,
    pub current_tab: Option<CurrentTab>,
    pub other_tabs: HashMap<u32, StateSnapshotPayload>,
    pub sync_requested: bool,
    pub home_dir: PathBuf,
    pub frame: Vec<TabRow>,
    pub last_cols: usize,
    pub render_buf: String,
}

impl State {
    pub fn current_tab_id(&self) -> Option<usize> {
        self.current_tab.as_ref().map(|t| t.tab_id)
    }

    pub fn ensure_current_tab(&mut self, manifest: &PaneManifest) -> bool {
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

    pub fn refresh_current_tab_from_manifest(
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
        let prev_focused_pane = current_tab.focused_pane.clone();

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

        sync_agent_by_pane_with_focused_pane(current_tab);
        clear_agent_by_pane_when_focused_agent_disappears(current_tab, prev_focused_pane.as_ref());
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

    pub fn update_current_tab_cwd(&mut self, pane_id: u32, cwd: PathBuf) -> bool {
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

    pub fn update_current_tab_agent_event(&mut self, event_payload: AgentEventPayload) -> (bool, bool) {
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

    pub fn update_current_tab_git_stat(
        &mut self,
        requested_cwd: &PathBuf,
        exit_code: Option<i32>,
        stdout: &[u8],
    ) -> bool {
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

    pub fn bump_current_tab_seq(&mut self) {
        if let Some(current_tab) = self.current_tab.as_mut() {
            current_tab.seq = current_tab.seq.saturating_add(1);
        }
    }

    pub fn send_sync_request(&mut self) {
        if self.current_tab.is_none() {
            return;
        }

        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "sync_request".to_string());
        pipe_message_to_plugin(MessageToPlugin::new(SYNC_PIPE.to_string()).with_args(args));
        self.sync_requested = true;
    }

    pub fn remap_current_tab_id_after_tab_update(&mut self, prev_tabs: &[TabInfo]) -> bool {
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

    pub fn apply_remote_snapshot(&mut self, source_plugin_id: u32, snapshot: StateSnapshotPayload) -> bool {
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

    pub fn prune_state_for_closed_tabs(&mut self) {
        let known_tabs: HashSet<usize> = self.all_tabs.iter().map(|tab| tab.tab_id).collect();
        self.other_tabs.retain(|_, remote| known_tabs.contains(&remote.tab_id));
    }

    pub fn remote_snapshot_for_tab(&self, tab_id: usize) -> Option<&StateSnapshotPayload> {
        self.other_tabs
            .values()
            .filter(|remote| remote.tab_id == tab_id)
            .max_by_key(|remote| remote.seq)
    }

    pub fn sync_frame(&mut self) -> bool {
        let next = compute_frame(self);
        if self.frame == next {
            return false;
        }
        self.frame = next;
        true
    }

    fn discover_current_tab_id(&self, manifest: &PaneManifest) -> Option<usize> {
        let tab_pos = self.current_tab_position_in_manifest(manifest)?;
        self.all_tabs
            .iter()
            .find(|tab| tab.position == tab_pos)
            .map(|tab| tab.tab_id)
    }

    fn current_tab_position_in_manifest(&self, manifest: &PaneManifest) -> Option<usize> {
        manifest.panes.iter().find_map(|(tab_pos, panes)| {
            panes
                .iter()
                .any(|pane| pane.is_plugin && pane.id == self.plugin_id)
                .then_some(*tab_pos)
        })
    }
}

#[cfg_attr(test, derive(Debug, PartialEq))]
#[derive(Default)]
pub struct CurrentTab {
    pub tab_id: usize,
    pub seq: u64,
    pub pane_ids: HashSet<u32>,
    pub focused_pane: Option<FocusedPane>,
    pub cwd: Option<PathBuf>,
    pub agent_by_pane: HashMap<u32, Cmd>,
    pub git_stat: GitStat,
}

impl CurrentTab {
    pub fn new(tab_id: usize) -> Self {
        Self {
            tab_id,
            ..Default::default()
        }
    }

    pub fn cmd(&self) -> Cmd {
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

#[derive(Clone, Default, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub struct FocusedPane {
    id: u32,
    cmd: Option<String>,
}

fn clear_agent_by_pane_when_focused_agent_disappears(
    current_tab: &mut CurrentTab,
    prev_focused_pane: Option<&FocusedPane>,
) {
    let Some(prev_focused_pane) = prev_focused_pane else {
        return;
    };
    let Some(current_focused_pane) = current_tab.focused_pane.as_ref() else {
        return;
    };
    if prev_focused_pane.id != current_focused_pane.id {
        return;
    }
    if prev_focused_pane.cmd.as_deref().and_then(Agent::detect).is_none() {
        return;
    }
    if current_focused_pane.cmd.as_deref().and_then(Agent::detect).is_some() {
        return;
    }
    current_tab.agent_by_pane.remove(&current_focused_pane.id);
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

/// Persist an agent once we have explicitly seen it in the focused pane.
///
/// This lets the tab keep showing that agent after focus moves to another pane, even if no hook
/// has fired yet.
fn sync_agent_by_pane_with_focused_pane(current_tab: &mut CurrentTab) {
    let Some(focused_pane) = current_tab.focused_pane.as_ref() else {
        return;
    };
    let Some(agent) = focused_pane.cmd.as_deref().and_then(Agent::detect) else {
        return;
    };
    match current_tab.agent_by_pane.get(&focused_pane.id) {
        Some(Cmd::BusyAgent(current) | Cmd::IdleAgent(current)) if *current == agent => {}
        _ => {
            current_tab.agent_by_pane.insert(focused_pane.id, Cmd::IdleAgent(agent));
        }
    }
}

/// Drop hook-driven agent state when we have high-confidence evidence that the pane no longer runs
/// that agent (e.g. focused Codex pane quit → shell).
///
/// For unfocused panes, Zellij manifest metadata can fall back to shell/path information even
/// while an agent TUI is still open. Clearing state from that data makes the agent disappear from
/// the tab as soon as focus moves elsewhere, so only focused panes are reconciled against command
/// metadata.
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

        if !pane.is_focused {
            continue;
        }

        match detected {
            Some(d) if d != stored => {
                current_tab.agent_by_pane.remove(&pane_id);
            }
            None if pane.terminal_command.as_ref().is_some_and(|s| !s.trim().is_empty()) => {
                current_tab.agent_by_pane.remove(&pane_id);
            }
            _ => {}
        }
    }
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
    fn refresh_current_tab_from_manifest_clears_agent_state_when_focused_pane_title_becomes_path() {
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
        current_tab.agent_by_pane.insert(42, Cmd::IdleAgent(Agent::Codex));

        let pane = terminal_pane_with_title(42, true, "/tmp/project");
        let manifest = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let _ = state.refresh_current_tab_from_manifest(&manifest, noop_pane_cwd);

        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        pretty_assertions::assert_eq!(current_tab.agent_by_pane.get(&42), None);
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::None);
    }

    #[test]
    fn refresh_current_tab_from_manifest_keeps_agent_state_for_unfocused_shell_metadata() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let current_tab = state.current_tab.as_mut().expect("missing current tab");
        current_tab.pane_ids.extend([42, 43]);
        current_tab.focused_pane = Some(FocusedPane {
            id: 43,
            cmd: Some("cargo".to_string()),
        });
        current_tab.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        let manifest = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, false, "/bin/zsh"),
                terminal_pane_with_command(43, true, "/usr/bin/cargo test"),
            ],
        )]);
        let _ = state.refresh_current_tab_from_manifest(&manifest, noop_pane_cwd);

        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        pretty_assertions::assert_eq!(current_tab.agent_by_pane.get(&42), Some(&Cmd::BusyAgent(Agent::Codex)));
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::BusyAgent(Agent::Codex));
    }

    #[test]
    fn refresh_current_tab_from_manifest_persists_agent_seen_in_focused_pane() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let first_manifest = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, true, "/opt/homebrew/bin/codex"),
            ],
        )]);
        let _ = state.refresh_current_tab_from_manifest(&first_manifest, noop_pane_cwd);

        let second_manifest = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_title(42, false, "/tmp/project"),
                terminal_pane_with_command(43, true, "/usr/bin/cargo test"),
            ],
        )]);
        let _ = state.refresh_current_tab_from_manifest(&second_manifest, noop_pane_cwd);

        let current_tab = state.current_tab.as_ref().expect("missing current tab");
        pretty_assertions::assert_eq!(current_tab.agent_by_pane.get(&42), Some(&Cmd::IdleAgent(Agent::Codex)));
        pretty_assertions::assert_eq!(current_tab.cmd(), Cmd::IdleAgent(Agent::Codex));
    }

    #[test]
    fn parse_pane_title_ignores_shell_names() {
        pretty_assertions::assert_eq!(parse_pane_title("zsh"), None);
        pretty_assertions::assert_eq!(parse_pane_title("bash"), None);
        pretty_assertions::assert_eq!(parse_pane_title("fish"), None);
        pretty_assertions::assert_eq!(parse_pane_title("cargo test"), Some("cargo".to_string()));
    }
}
