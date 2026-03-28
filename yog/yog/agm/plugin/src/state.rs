use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use agm_core::Agent;
use agm_core::AgentEventKind;
use agm_core::AgentEventPayload;
use agm_core::Cmd;
use agm_core::GitStat;
use zellij_tile::prelude::*;

use crate::StateSnapshotPayload;
use crate::ui::TabRow;

// ── Domain events ─────────────────────────────────────────────────────────────

#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub enum StateEvent {
    // Current-tab identity
    TabCreated {
        tab_id: usize,
    },
    TabRemapped {
        new_tab_id: usize,
    },

    // Pane layout
    PanesChanged {
        new_pane_ids: HashSet<u32>,
    },
    FocusMoved {
        new_pane: Option<FocusedPane>,
    },

    // Working directory
    CwdChanged {
        new_cwd: PathBuf,
    },

    // Agent lifecycle
    /// First detection of an agent in a pane (idle by default).
    AgentDetected {
        pane_id: u32,
        agent: Agent,
    },
    AgentBusy {
        pane_id: u32,
        agent: Agent,
    },
    /// Agent finished processing — also implies a git refresh.
    AgentIdle {
        pane_id: u32,
        agent: Agent,
    },
    /// Agent exited, pane closed, or process replaced.
    AgentLost {
        pane_id: u32,
    },

    // Git statistics
    GitStatChanged {
        new_stat: GitStat,
    },

    // Remote tab display (other plugin instances)
    RemoteTabUpdated {
        source_plugin_id: u32,
        snapshot: StateSnapshotPayload,
        evict_ids: Vec<u32>,
    },

    // Tab bar topology
    TopologyChanged,
    /// Current tab just became Zellij's active tab.
    BecameActive,

    // Tab list management
    /// Full replacement of the tab list (from a `TabUpdate` Zellij event).
    /// Apply sets `self.all_tabs` and prunes closed remote tabs.
    AllTabsReplaced {
        new_tabs: Vec<TabInfo>,
    },
    /// A sync request pipe message should be sent to peer plugin instances.
    /// Apply sets `self.sync_requested = true`.
    SyncRequested,
}

// ── State ─────────────────────────────────────────────────────────────────────

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

    // ── Event derivation (pure, &self) ────────────────────────────────────────

    /// Derives state events from a `PaneUpdate` Zellij event.
    pub fn events_from_pane_update(
        &self,
        manifest: &PaneManifest,
        mut resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
    ) -> Vec<StateEvent> {
        let Some(tab_pos) = self.current_tab_position_in_manifest(manifest) else {
            return vec![];
        };
        let Some(panes) = manifest.panes.get(&tab_pos) else {
            return vec![];
        };

        let mut events = vec![];

        // 1. Tab identity
        let current_tab_id = self.current_tab.as_ref().map(|ct| ct.tab_id);
        let discovered_tab_id = self.all_tabs.iter().find(|t| t.position == tab_pos).map(|t| t.tab_id);

        match (current_tab_id, discovered_tab_id) {
            (None, Some(tab_id)) => {
                events.push(StateEvent::TabCreated { tab_id });
            }
            (Some(current_id), Some(discovered_id)) if !self.all_tabs.iter().any(|t| t.tab_id == current_id) => {
                events.push(StateEvent::TabRemapped {
                    new_tab_id: discovered_id,
                });
            }
            _ => {}
        }

        // Resolve current tab state (after potential identity event)
        let effective_tab_id = match events.first() {
            Some(StateEvent::TabCreated { tab_id }) => *tab_id,
            Some(StateEvent::TabRemapped { new_tab_id }) => *new_tab_id,
            _ => match current_tab_id {
                Some(id) => id,
                None => return events, // no tab, nothing more to do
            },
        };
        let _ = effective_tab_id; // used implicitly via current_tab below

        let current_tab = match self.current_tab.as_ref() {
            Some(ct) => ct,
            None => {
                // Tab just created — nothing else to diff against
                return events;
            }
        };

        // 2. Pane set
        let mut new_pane_ids = HashSet::new();
        let mut new_focused_pane: Option<FocusedPane> = None;
        for pane in panes.iter().filter(|p| !p.is_plugin && !p.exited && !p.is_held) {
            new_pane_ids.insert(pane.id);
            if pane.is_focused {
                new_focused_pane = Some(FocusedPane {
                    id: pane.id,
                    cmd: focused_pane_running_command(pane),
                });
            }
        }

        if new_pane_ids != current_tab.pane_ids {
            // AgentLost for removed panes
            for removed in current_tab.pane_ids.difference(&new_pane_ids) {
                if current_tab.agent_by_pane.contains_key(removed) {
                    events.push(StateEvent::AgentLost { pane_id: *removed });
                }
            }
            events.push(StateEvent::PanesChanged {
                new_pane_ids: new_pane_ids.clone(),
            });
        }

        // 3. Focus
        let focused_changed = new_focused_pane != current_tab.focused_pane;
        if focused_changed {
            events.push(StateEvent::FocusMoved {
                new_pane: new_focused_pane.clone(),
            });
        }

        // 4. Agent reconciliation
        events.extend(self.agent_events_from_manifest(
            current_tab,
            current_tab.focused_pane.as_ref(),
            new_focused_pane.as_ref(),
            panes,
            &new_pane_ids,
        ));

        // 5. CWD
        if let Some(focused) = new_focused_pane.as_ref()
            && (focused_changed || current_tab.cwd.is_none())
            && let Some(new_cwd) = resolve_pane_cwd(focused.id)
            && current_tab.cwd.as_ref() != Some(&new_cwd)
        {
            events.push(StateEvent::CwdChanged { new_cwd });
        }

        // 6. Sync request: send when tab was just created/remapped, or hasn't been requested yet.
        let has_resetter = events
            .iter()
            .any(|e| matches!(e, StateEvent::TabCreated { .. } | StateEvent::TabRemapped { .. }));
        if has_resetter || (self.current_tab.is_some() && !self.sync_requested) {
            events.push(StateEvent::SyncRequested);
        }

        events
    }

    /// Derives state events from a `TabUpdate` Zellij event.
    /// `new_tabs` is the incoming sorted tab list; `self.all_tabs` is the previous list.
    pub fn events_from_tab_update(&self, new_tabs: &mut [TabInfo]) -> Vec<StateEvent> {
        new_tabs.sort_by_key(|tab| tab.position);

        let prev_tabs = &self.all_tabs;
        let mut events = vec![];

        events.push(StateEvent::AllTabsReplaced {
            new_tabs: new_tabs.to_vec(),
        });

        let topology_changed = topology_changed(prev_tabs, new_tabs);
        if topology_changed {
            events.push(StateEvent::TopologyChanged);
        }

        // Was the current tab active before? Is it active now?
        let was_active = self
            .current_tab_id()
            .is_some_and(|id| prev_tabs.iter().any(|t| t.active && t.tab_id == id));
        let is_active = self
            .current_tab_id()
            .is_some_and(|id| new_tabs.iter().any(|t| t.active && t.tab_id == id));
        if !was_active && is_active {
            events.push(StateEvent::BecameActive);
        }

        // Remap: current tab_id gone, can we find it in the new list?
        let has_remap = if let Some(new_id) = detect_remapped_tab_id(self.current_tab.as_ref(), prev_tabs, new_tabs) {
            events.push(StateEvent::TabRemapped { new_tab_id: new_id });
            true
        } else {
            false
        };

        // Sync request: needed when topology resets the sync flag, or it was never sent.
        if self.current_tab.is_some() && (!self.sync_requested || topology_changed || has_remap) {
            events.push(StateEvent::SyncRequested);
        }

        events
    }

    /// Derives state events from a `CwdChanged` Zellij event.
    pub fn events_from_cwd_changed(&self, pane_id: u32, cwd: PathBuf) -> Vec<StateEvent> {
        let Some(ct) = self.current_tab.as_ref() else {
            return vec![];
        };
        if ct.focused_pane.as_ref().map(|fp| fp.id) != Some(pane_id) {
            return vec![];
        }
        if ct.cwd.as_ref() == Some(&cwd) {
            return vec![];
        }
        vec![StateEvent::CwdChanged { new_cwd: cwd }]
    }

    /// Derives state events from a `RunCommandResult` Zellij event (git-stat subprocess).
    pub fn events_from_run_command_result(
        &self,
        requested_cwd: &PathBuf,
        exit_code: Option<i32>,
        stdout: &[u8],
    ) -> Vec<StateEvent> {
        if exit_code != Some(0) {
            return vec![];
        }
        let Some(ct) = self.current_tab.as_ref() else {
            return vec![];
        };
        if ct.cwd.as_ref() != Some(requested_cwd) {
            return vec![];
        }

        let output = String::from_utf8_lossy(stdout);
        for line in output.lines() {
            let Ok((path, new_stat)) = GitStat::parse_line(line).inspect_err(|e| eprintln!("agm: {e}")) else {
                continue;
            };
            if path != *requested_cwd {
                continue;
            }
            if ct.git_stat == new_stat {
                return vec![];
            }
            return vec![StateEvent::GitStatChanged { new_stat }];
        }
        vec![]
    }

    /// Derives state events from an `Agent` pipe event.
    pub fn events_from_agent_event(&self, event: &AgentEventPayload) -> Vec<StateEvent> {
        let Some(ct) = self.current_tab.as_ref() else {
            return vec![];
        };
        if !ct.pane_ids.contains(&event.pane_id) {
            return vec![];
        }

        // Priority guard: don't apply a lower-priority agent over a higher-priority one.
        let current = ct.agent_by_pane.get(&event.pane_id);
        if current
            .and_then(|c| c.agent_name())
            .and_then(|n| Agent::from_name(n).ok())
            .is_some_and(|current_agent| event.agent.priority() < current_agent.priority())
        {
            return vec![];
        }

        let pane_id = event.pane_id;
        let agent = event.agent;
        match event.kind {
            AgentEventKind::Start => {
                if matches!(current, Some(Cmd::IdleAgent(a)) if *a == agent) {
                    return vec![];
                }
                vec![StateEvent::AgentDetected { pane_id, agent }]
            }
            AgentEventKind::Busy => {
                if matches!(current, Some(Cmd::BusyAgent(a)) if *a == agent) {
                    return vec![];
                }
                vec![StateEvent::AgentBusy { pane_id, agent }]
            }
            AgentEventKind::Idle => {
                if matches!(current, Some(Cmd::IdleAgent(a)) if *a == agent) {
                    return vec![];
                }
                vec![StateEvent::AgentIdle { pane_id, agent }]
            }
            AgentEventKind::Exit => {
                if current.is_none() {
                    return vec![];
                }
                vec![StateEvent::AgentLost { pane_id }]
            }
        }
    }

    /// Derives state events from a `StateSnapshot` pipe event (remote tab sync).
    pub fn events_from_state_snapshot(
        &self,
        source_plugin_id: u32,
        snapshot: &StateSnapshotPayload,
    ) -> Vec<StateEvent> {
        if source_plugin_id == self.plugin_id
            || self.current_tab_id() == Some(snapshot.tab_id)
            || !self.all_tabs.iter().any(|t| t.tab_id == snapshot.tab_id)
            || self
                .other_tabs
                .get(&source_plugin_id)
                .is_some_and(|remote| snapshot.seq <= remote.seq)
        {
            return vec![];
        }

        let evict_ids: Vec<u32> = self
            .other_tabs
            .iter()
            .filter(|&(id, remote)| *id != source_plugin_id && remote.tab_id == snapshot.tab_id)
            .map(|(&id, _)| id)
            .collect();

        vec![StateEvent::RemoteTabUpdated {
            source_plugin_id,
            snapshot: snapshot.clone(),
            evict_ids,
        }]
    }

    // ── Apply (all state mutations) ───────────────────────────────────────────

    fn apply(&mut self, event: &StateEvent) {
        match event {
            StateEvent::TabCreated { tab_id } => {
                self.current_tab = Some(CurrentTab::new(*tab_id));
            }
            StateEvent::TabRemapped { new_tab_id } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.tab_id = *new_tab_id;
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::PanesChanged { new_pane_ids } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.pane_ids = new_pane_ids.clone();
                    ct.agent_by_pane.retain(|id, _| new_pane_ids.contains(id));
                    if ct
                        .last_focused_agent_pane_id
                        .is_some_and(|pane_id| !new_pane_ids.contains(&pane_id))
                    {
                        ct.last_focused_agent_pane_id = None;
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::FocusMoved { new_pane } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.focused_pane = new_pane.clone();
                    if let Some(pane) = new_pane.as_ref()
                        && ct.agent_by_pane.contains_key(&pane.id)
                    {
                        ct.last_focused_agent_pane_id = Some(pane.id);
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::CwdChanged { new_cwd } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.cwd = Some(new_cwd.clone());
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::AgentDetected { pane_id, agent } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.agent_by_pane.entry(*pane_id).or_insert(Cmd::IdleAgent(*agent));
                    if ct.focused_pane.as_ref().map(|pane| pane.id) == Some(*pane_id) {
                        ct.last_focused_agent_pane_id = Some(*pane_id);
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::AgentBusy { pane_id, agent } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.agent_by_pane.insert(*pane_id, Cmd::BusyAgent(*agent));
                    if ct.focused_pane.as_ref().map(|pane| pane.id) == Some(*pane_id) {
                        ct.last_focused_agent_pane_id = Some(*pane_id);
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::AgentIdle { pane_id, agent } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.agent_by_pane.insert(*pane_id, Cmd::IdleAgent(*agent));
                    if ct.focused_pane.as_ref().map(|pane| pane.id) == Some(*pane_id) {
                        ct.last_focused_agent_pane_id = Some(*pane_id);
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::AgentLost { pane_id } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.agent_by_pane.remove(pane_id);
                    if ct.last_focused_agent_pane_id == Some(*pane_id) {
                        ct.last_focused_agent_pane_id = None;
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::GitStatChanged { new_stat } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.git_stat = *new_stat;
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::TopologyChanged => {}
            StateEvent::AllTabsReplaced { new_tabs } => {
                let known: HashSet<usize> = new_tabs.iter().map(|t| t.tab_id).collect();
                self.other_tabs.retain(|_, remote| known.contains(&remote.tab_id));
                self.all_tabs = new_tabs.clone();
            }
            StateEvent::SyncRequested => {
                self.sync_requested = true;
            }
            StateEvent::RemoteTabUpdated {
                source_plugin_id,
                snapshot,
                evict_ids,
            } => {
                for id in evict_ids {
                    self.other_tabs.remove(id);
                }
                self.other_tabs.insert(*source_plugin_id, snapshot.clone());
            }
            StateEvent::BecameActive => {
                // IO-only signal, no state to update.
            }
        }
    }

    /// Apply all events, bump seq if any require a snapshot, recompute frame.
    /// Returns whether the frame changed (Zellij rerender signal).
    pub fn apply_all(&mut self, events: &[StateEvent]) -> bool {
        for event in events {
            self.apply(event);
        }
        self.sync_frame()
    }

    // ── Remaining public helpers ──────────────────────────────────────────────

    pub fn remote_snapshot_for_tab(&self, tab_id: usize) -> Option<&StateSnapshotPayload> {
        self.other_tabs
            .values()
            .filter(|r| r.tab_id == tab_id)
            .max_by_key(|r| r.seq)
    }

    pub fn sync_frame(&mut self) -> bool {
        let next = compute_frame(self);
        if self.frame == next {
            return false;
        }
        self.frame = next;
        true
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn current_tab_position_in_manifest(&self, manifest: &PaneManifest) -> Option<usize> {
        manifest.panes.iter().find_map(|(tab_pos, panes)| {
            panes
                .iter()
                .any(|p| p.is_plugin && p.id == self.plugin_id)
                .then_some(*tab_pos)
        })
    }

    /// Pure: computes agent state changes implied by the new manifest.
    /// Replaces the three former mutating helpers.
    fn agent_events_from_manifest(
        &self,
        current_tab: &CurrentTab,
        prev_focused_pane: Option<&FocusedPane>,
        new_focused_pane: Option<&FocusedPane>,
        panes: &[PaneInfo],
        surviving_pane_ids: &HashSet<u32>,
    ) -> Vec<StateEvent> {
        let mut events = vec![];

        // Detect new agent in the newly-focused pane (persist across focus moves).
        if let Some(focused) = new_focused_pane
            && let Some(agent) = focused.cmd.as_deref().and_then(Agent::detect)
        {
            match current_tab.agent_by_pane.get(&focused.id) {
                Some(Cmd::BusyAgent(a) | Cmd::IdleAgent(a)) if *a == agent => {}
                _ => {
                    events.push(StateEvent::AgentDetected {
                        pane_id: focused.id,
                        agent,
                    });
                }
            }
        }

        // Clear agent state when the focused pane no longer shows that agent.
        if let Some(prev) = prev_focused_pane
            && let Some(new) = new_focused_pane
            && prev.id == new.id
            && prev.cmd.as_deref().and_then(Agent::detect).is_some()
            && new.cmd.as_deref().and_then(Agent::detect).is_none()
        {
            events.push(StateEvent::AgentLost { pane_id: new.id });
        }

        // Reconcile focused pane against manifest command metadata.
        for (&pane_id, cmd) in &current_tab.agent_by_pane {
            if !surviving_pane_ids.contains(&pane_id) {
                continue; // already emitted AgentLost in events_from_pane_update
            }
            let Some(stored_agent) = cmd.agent_name().and_then(|n| Agent::from_name(n).ok()) else {
                continue;
            };
            let Some(pane) = panes.iter().find(|p| p.id == pane_id && !p.is_plugin) else {
                continue;
            };
            if pane.exited || pane.is_held || !pane.is_focused {
                continue; // only reconcile focused panes
            }
            let detected = focused_pane_running_command(pane).and_then(|exe| Agent::detect(&exe));
            let should_clear = match detected {
                Some(d) if d != stored_agent => true,
                None if pane.terminal_command.as_ref().is_some_and(|s| !s.trim().is_empty()) => true,
                _ => false,
            };
            if should_clear {
                events.push(StateEvent::AgentLost { pane_id });
            }
        }

        events
    }
}

// ── CurrentTab ────────────────────────────────────────────────────────────────

#[cfg_attr(test, derive(Debug, PartialEq))]
#[derive(Default)]
pub struct CurrentTab {
    pub tab_id: usize,
    pub seq: u64,
    pub pane_ids: HashSet<u32>,
    pub focused_pane: Option<FocusedPane>,
    pub last_focused_agent_pane_id: Option<u32>,
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
        compute_cmd(
            &self.agent_by_pane,
            self.focused_pane.as_ref(),
            self.last_focused_agent_pane_id,
        )
    }
}

// ── FocusedPane ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FocusedPane {
    pub id: u32,
    pub cmd: Option<String>,
}

// ── Frame computation ─────────────────────────────────────────────────────────

fn compute_frame(state: &State) -> Vec<TabRow> {
    state
        .all_tabs
        .iter()
        .map(|tab| {
            if state.current_tab_id() == Some(tab.tab_id)
                && let Some(ct) = state.current_tab.as_ref()
            {
                return TabRow::new(tab, ct.cwd.as_ref(), ct.cmd(), ct.git_stat, state.home_dir.as_path());
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

// ── Pure helpers ──────────────────────────────────────────────────────────────

/// The effective command for a tab:
/// - focused agent wins
/// - otherwise a single tracked agent wins
/// - otherwise the last focused tracked agent wins
/// - otherwise fall back to the focused pane command
pub fn compute_cmd(
    agent_by_pane: &HashMap<u32, Cmd>,
    focused_pane: Option<&FocusedPane>,
    last_focused_agent_pane_id: Option<u32>,
) -> Cmd {
    if let Some(focused_id) = focused_pane.map(|pane| pane.id)
        && let Some(cmd @ (Cmd::BusyAgent(_) | Cmd::IdleAgent(_))) = agent_by_pane.get(&focused_id)
    {
        return cmd.clone();
    }

    if let Some(focused) = focused_pane
        && let Some(cmd_line) = focused.cmd.as_deref()
    {
        if let Some(agent) = Agent::detect(cmd_line) {
            return Cmd::IdleAgent(agent);
        }
        if let Some(exe) = parse_running_command(cmd_line)
            && let Some(agent) = Agent::detect(&exe)
        {
            return Cmd::IdleAgent(agent);
        }
    }

    if agent_by_pane.len() == 1
        && let Some(cmd @ (Cmd::BusyAgent(_) | Cmd::IdleAgent(_))) = agent_by_pane.values().next()
    {
        return cmd.clone();
    }

    if let Some(last_focused_agent_pane_id) = last_focused_agent_pane_id
        && let Some(cmd @ (Cmd::BusyAgent(_) | Cmd::IdleAgent(_))) = agent_by_pane.get(&last_focused_agent_pane_id)
    {
        return cmd.clone();
    }

    focused_pane
        .and_then(|focused| focused.cmd.as_ref().map(|cmd_line| Cmd::Running(cmd_line.to_string())))
        .unwrap_or(Cmd::None)
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
    if trimmed.is_empty()
        || trimmed.starts_with('~')
        || trimmed.starts_with('/')
        || trimmed == "Pane"
        || trimmed.starts_with("Pane ")
    {
        return None;
    }
    if Agent::detect(trimmed).is_some() {
        return Some(trimmed.to_string());
    }
    parse_running_command(trimmed)
}

fn topology_changed(x: &[TabInfo], y: &[TabInfo]) -> bool {
    if x.len() != y.len() {
        return true;
    }
    x.iter()
        .zip(y.iter())
        .any(|(a, b)| a.tab_id != b.tab_id || a.position != b.position)
}

/// Pure core of tab-id remapping after a tab move/rename.
fn detect_remapped_tab_id(
    current_tab: Option<&CurrentTab>,
    prev_tabs: &[TabInfo],
    new_tabs: &[TabInfo],
) -> Option<usize> {
    let current_tab = current_tab?;
    if new_tabs.iter().any(|t| t.tab_id == current_tab.tab_id) {
        return None; // tab_id still present, no remap needed
    }

    let prev_ids: HashSet<usize> = prev_tabs.iter().map(|t| t.tab_id).collect();
    let next_ids: HashSet<usize> = new_tabs.iter().map(|t| t.tab_id).collect();
    let removed: HashSet<usize> = prev_ids.difference(&next_ids).copied().collect();
    if !removed.contains(&current_tab.tab_id) {
        return None;
    }

    let mut added: Vec<&TabInfo> = new_tabs.iter().filter(|t| !prev_ids.contains(&t.tab_id)).collect();
    if added.is_empty() {
        return None;
    }
    if added.len() > 1
        && let Some(prev_current) = prev_tabs.iter().find(|t| t.tab_id == current_tab.tab_id)
    {
        let by_name: Vec<_> = added.iter().copied().filter(|t| t.name == prev_current.name).collect();
        if by_name.len() == 1 {
            added = by_name;
        }
    }
    if added.len() == 1 { Some(added[0].tab_id) } else { None }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;

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
        PaneManifest {
            panes: entries.into_iter().collect(),
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

    fn apply_pane_update(state: &mut State, manifest: &PaneManifest) -> Vec<StateEvent> {
        let events = state.events_from_pane_update(manifest, noop_pane_cwd);
        state.apply_all(&events);
        events
    }

    #[test]
    fn test_pane_update_before_tab_update_does_not_rebind_current_tab_id() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a"), tab_with_name(11, 1, "b")],
            ..Default::default()
        };

        let initial_manifest = manifest(vec![(0, vec![plugin_pane(7), terminal_pane(42, true)])]);
        let events = apply_pane_update(&mut state, &initial_manifest);
        pretty_assertions::assert_eq!(events, vec![StateEvent::TabCreated { tab_id: 10 }]);

        let ct = state.current_tab.as_mut().unwrap();
        ct.cwd = Some(PathBuf::from("/tmp/project"));
        ct.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        // Simulate a tab move where PaneUpdate arrives before TabUpdate:
        // manifest already reflects new position, while all_tabs is still stale.
        let moved_manifest = manifest(vec![(1, vec![plugin_pane(7), terminal_pane(42, true)])]);
        let events2 = apply_pane_update(&mut state, &moved_manifest);
        pretty_assertions::assert_eq!(
            events2,
            vec![
                StateEvent::PanesChanged {
                    new_pane_ids: [42].into_iter().collect(),
                },
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane { id: 42, cmd: None }),
                },
                StateEvent::SyncRequested,
            ]
        );

        let current_tab = state.current_tab.as_ref().unwrap();
        let expected = CurrentTab {
            tab_id: 10,
            seq: 2, // bumped for PanesChanged and again for FocusMoved
            pane_ids: [42].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 42, cmd: None }),
            last_focused_agent_pane_id: Some(42),
            cwd: Some(PathBuf::from("/tmp/project")),
            agent_by_pane: [(42, Cmd::BusyAgent(Agent::Codex))].into_iter().collect(),
            git_stat: GitStat::default(),
        };
        pretty_assertions::assert_eq!(current_tab, &expected);
    }

    #[test]
    fn test_pane_update_rebinds_current_tab_id_only_after_old_id_disappears() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(11, 0, "b"), tab_with_name(99, 1, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane { id: 42, cmd: None });
        ct.cwd = Some(PathBuf::from("/tmp/project"));
        ct.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        // Tab 10 is not in all_tabs; plugin is at position 1 → tab 99
        let m = manifest(vec![(1, vec![plugin_pane(7), terminal_pane(42, true)])]);
        let events = apply_pane_update(&mut state, &m);
        pretty_assertions::assert_eq!(
            events,
            vec![StateEvent::TabRemapped { new_tab_id: 99 }, StateEvent::SyncRequested,]
        );

        let current_tab = state.current_tab.as_ref().unwrap();
        let expected = CurrentTab {
            tab_id: 99,
            seq: 1, // bumped by apply_all because TabRemapped requires a snapshot
            pane_ids: [42].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 42, cmd: None }),
            last_focused_agent_pane_id: None,
            cwd: Some(PathBuf::from("/tmp/project")),
            agent_by_pane: [(42, Cmd::BusyAgent(Agent::Codex))].into_iter().collect(),
            git_stat: GitStat::default(),
        };
        pretty_assertions::assert_eq!(current_tab, &expected);
    }

    #[test]
    fn test_remap_current_tab_id_after_tab_update_can_use_tab_name_when_many_ids_changed() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(10)),
            all_tabs: vec![tab_with_name(10, 0, "agent"), tab_with_name(20, 1, "shell")],
            ..Default::default()
        };
        let mut new_tabs = vec![tab_with_name(30, 1, "agent"), tab_with_name(40, 0, "shell")];
        let events = state.events_from_tab_update(&mut new_tabs);
        state.apply_all(&events);
        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::AllTabsReplaced {
                    new_tabs: new_tabs.to_vec(),
                },
                StateEvent::TopologyChanged,
                StateEvent::TabRemapped { new_tab_id: 30 },
                StateEvent::SyncRequested,
            ]
        );
        pretty_assertions::assert_eq!(
            state.current_tab.as_ref().unwrap(),
            &CurrentTab {
                tab_id: 30,
                seq: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_remote_snapshot_sequence_is_tracked_per_source_plugin() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(1)),
            all_tabs: vec![tab_with_name(1, 0, "local"), tab_with_name(2, 1, "remote")],
            ..Default::default()
        };

        let snap1 = snapshot(2, 10, Cmd::BusyAgent(Agent::Codex));
        let events1 = state.events_from_state_snapshot(100, &snap1);
        state.apply_all(&events1);

        let snap2 = snapshot(2, 1, Cmd::IdleAgent(Agent::Claude));
        let events2 = state.events_from_state_snapshot(200, &snap2);
        state.apply_all(&events2);

        let expected = HashMap::from([(200, snapshot(2, 1, Cmd::IdleAgent(Agent::Claude)))]);
        pretty_assertions::assert_eq!(state.other_tabs, expected);
    }

    #[rstest]
    #[case::fallback_to_focused_non_agent(
        vec![],
        Some((0, "cargo")),
        None,
        Cmd::Running("cargo".to_string())
    )]
    #[case::prioritizes_agents_over_focused_non_agent(
        vec![(42, Cmd::BusyAgent(Agent::Codex))],
        Some((0, "cargo")),
        None,
        Cmd::BusyAgent(Agent::Codex)
    )]
    #[case::prefers_focused_agent_over_higher_priority_unfocused(
        vec![(42, Cmd::BusyAgent(Agent::Claude)), (43, Cmd::BusyAgent(Agent::Cursor))],
        Some((42, "claude")),
        Some(42),
        Cmd::BusyAgent(Agent::Claude)
    )]
    #[case::keeps_single_agent_visible_when_focus_moves_to_non_agent(
        vec![(42, Cmd::IdleAgent(Agent::Claude))],
        Some((43, "cargo")),
        Some(42),
        Cmd::IdleAgent(Agent::Claude)
    )]
    #[case::prefers_visible_focused_agent_over_single_tracked_agent(
        vec![(42, Cmd::BusyAgent(Agent::Claude))],
        Some((43, "Cursor Agent")),
        Some(42),
        Cmd::IdleAgent(Agent::Cursor)
    )]
    #[case::uses_last_focused_agent_when_multiple_agents_exist(
        vec![(42, Cmd::IdleAgent(Agent::Claude)), (43, Cmd::BusyAgent(Agent::Cursor))],
        Some((44, "cargo")),
        Some(42),
        Cmd::IdleAgent(Agent::Claude)
    )]
    #[case::shows_idle_agent_when_focus_executable_matches_agent(
        vec![],
        Some((0, "/opt/homebrew/bin/codex")),
        None,
        Cmd::IdleAgent(Agent::Codex)
    )]
    fn test_test_compute_cmd(
        #[case] agents: Vec<(u32, Cmd)>,
        #[case] focused: Option<(u32, &str)>,
        #[case] last_focused_agent_id: Option<u32>,
        #[case] expected: Cmd,
    ) {
        let agent_by_pane: HashMap<_, _> = agents.into_iter().collect();
        let focused_pane = focused.map(|(id, cmd)| FocusedPane {
            id,
            cmd: Some(cmd.to_string()),
        });

        pretty_assertions::assert_eq!(
            compute_cmd(&agent_by_pane, focused_pane.as_ref(), last_focused_agent_id),
            expected
        );
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_picks_focused_command_name() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane { id: 42, cmd: None });
        ct.cwd = Some(PathBuf::from("/tmp/project"));

        let pane = terminal_pane_with_command(42, true, "/usr/bin/cargo test -p agm-plugin");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        cmd: Some("cargo".to_string())
                    })
                },
                StateEvent::SyncRequested,
            ]
        );
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::Running("cargo".to_string()));
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_falls_back_to_focused_pane_title() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane { id: 42, cmd: None });
        ct.cwd = Some(PathBuf::from("/tmp/project"));

        let pane = terminal_pane_with_title(42, true, "nvim");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        cmd: Some("nvim".to_string())
                    })
                },
                StateEvent::SyncRequested,
            ]
        );
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::Running("nvim".to_string()));
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_ignores_shell_or_path_titles() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            cmd: Some("nvim".to_string()),
        });
        ct.cwd = Some(PathBuf::from("/tmp/project"));

        let pane = terminal_pane_with_title(42, true, "/tmp/project");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane { id: 42, cmd: None })
                },
                StateEvent::SyncRequested,
            ]
        );
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::None);
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_clears_agent_state_when_pane_process_changes() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            cmd: Some("codex".to_string()),
        });
        ct.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        let pane = terminal_pane_with_command(42, true, "/bin/zsh");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane { id: 42, cmd: None })
                },
                StateEvent::AgentLost { pane_id: 42 },
                StateEvent::AgentLost { pane_id: 42 },
                StateEvent::SyncRequested,
            ]
        );

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.agent_by_pane.get(&42), None);
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::None);
    }

    #[test]
    fn test_refrush_current_tab_from_manifest_clears_agent_state_when_focused_pane_title_becomes_path() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            cmd: Some("codex".to_string()),
        });
        ct.agent_by_pane.insert(42, Cmd::IdleAgent(Agent::Codex));

        let pane = terminal_pane_with_title(42, true, "/tmp/project");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane { id: 42, cmd: None })
                },
                StateEvent::AgentLost { pane_id: 42 },
                StateEvent::SyncRequested,
            ]
        );

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.agent_by_pane.get(&42), None);
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::None);
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_keeps_agent_state_for_unfocused_shell_metadata() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.extend([42, 43]);
        ct.focused_pane = Some(FocusedPane {
            id: 43,
            cmd: Some("cargo".to_string()),
        });
        ct.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Codex));

        let m = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, false, "/bin/zsh"),
                terminal_pane_with_command(43, true, "/usr/bin/cargo test"),
            ],
        )]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(events, vec![StateEvent::SyncRequested]);

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.agent_by_pane.get(&42), Some(&Cmd::BusyAgent(Agent::Codex)));
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::BusyAgent(Agent::Codex));
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_persists_agent_seen_in_focused_pane() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let first = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, true, "/opt/homebrew/bin/codex"),
            ],
        )]);
        let events1 = apply_pane_update(&mut state, &first);
        pretty_assertions::assert_eq!(
            events1,
            vec![
                StateEvent::PanesChanged {
                    new_pane_ids: [42].into_iter().collect()
                },
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        cmd: Some("codex".to_string())
                    })
                },
                StateEvent::AgentDetected {
                    pane_id: 42,
                    agent: Agent::Codex
                },
                StateEvent::SyncRequested,
            ]
        );

        let second = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_title(42, false, "/tmp/project"),
                terminal_pane_with_command(43, true, "/usr/bin/cargo test"),
            ],
        )]);
        let events2 = apply_pane_update(&mut state, &second);
        pretty_assertions::assert_eq!(
            events2,
            vec![
                StateEvent::PanesChanged {
                    new_pane_ids: [42, 43].into_iter().collect()
                },
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 43,
                        cmd: Some("cargo".to_string())
                    })
                },
            ]
        );

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.agent_by_pane.get(&42), Some(&Cmd::IdleAgent(Agent::Codex)));
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::IdleAgent(Agent::Codex));
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_prefers_last_focused_agent_when_multiple_agents_exist() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let first = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, true, "/opt/homebrew/bin/claude"),
                terminal_pane_with_command(43, false, "/usr/local/bin/cursor-agent"),
            ],
        )]);
        apply_pane_update(&mut state, &first);
        state.apply_all(&state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Claude,
            kind: AgentEventKind::Busy,
        }));
        state.apply_all(&state.events_from_agent_event(&AgentEventPayload {
            pane_id: 43,
            agent: Agent::Cursor,
            kind: AgentEventKind::Busy,
        }));

        let second = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, false, "/opt/homebrew/bin/claude"),
                terminal_pane_with_command(43, true, "/usr/local/bin/cursor-agent"),
            ],
        )]);
        apply_pane_update(&mut state, &second);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::BusyAgent(Agent::Cursor));

        let third = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, true, "/opt/homebrew/bin/claude"),
                terminal_pane_with_command(43, false, "/usr/local/bin/cursor-agent"),
            ],
        )]);
        apply_pane_update(&mut state, &third);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::BusyAgent(Agent::Claude));
    }

    #[test]
    fn test_focus_move_between_agent_panes_bumps_seq_without_new_agent_events() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.seq = 5;
        ct.pane_ids.extend([42, 43]);
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            cmd: Some("claude".to_string()),
        });
        ct.last_focused_agent_pane_id = Some(42);
        ct.agent_by_pane.insert(42, Cmd::BusyAgent(Agent::Claude));
        ct.agent_by_pane.insert(43, Cmd::BusyAgent(Agent::Cursor));

        let m = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, false, "/opt/homebrew/bin/claude"),
                terminal_pane_with_command(43, true, "/usr/local/bin/cursor-agent"),
            ],
        )]);
        let events = state.events_from_pane_update(&m, noop_pane_cwd);
        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 43,
                        cmd: Some("cursor-agent".to_string())
                    })
                },
                StateEvent::SyncRequested,
            ]
        );

        state.apply_all(&events);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.seq, 6);
        pretty_assertions::assert_eq!(ct.last_focused_agent_pane_id, Some(43));
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::BusyAgent(Agent::Cursor));
    }

    #[rstest]
    #[case("zsh", None)]
    #[case("bash", None)]
    #[case("fish", None)]
    #[case("cargo test", Some("cargo".to_string()))]
    #[case("✳ Claude Code", Some("✳ Claude Code".to_string()))]
    #[case("Cursor Agent", Some("Cursor Agent".to_string()))]
    fn test_parse_pane_title(#[case] input: &str, #[case] expected: Option<String>) {
        pretty_assertions::assert_eq!(parse_pane_title(input), expected);
    }
}
