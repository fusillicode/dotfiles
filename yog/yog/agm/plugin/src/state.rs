use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use agm_core::AgentState;
use agm_core::Cmd;
use agm_core::agent::Agent;
use agm_core::agent::AgentEventKind;
use agm_core::agent::AgentEventPayload;
use agm_core::git_stat::GitStat;
use zellij_tile::prelude::*;

use crate::StateSnapshotPayload;
use crate::events::StateEvent;
use crate::ui::TabRow;

#[derive(Default)]
pub struct State {
    pub plugin_id: u32,
    pub all_tabs: Vec<TabInfo>,
    pub current_tab: Option<CurrentTab>,
    pub other_tabs: HashMap<u32, StateSnapshotPayload>,
    pub known_active_tab_id: Option<usize>,
    pub sync_requested: bool,
    pub home_dir: PathBuf,
    pub frame: Vec<TabRow>,
    pub last_cols: usize,
    pub render_buf: String,
}

impl State {
    fn current_tab_is_active_in(tabs: &[TabInfo], current_tab_id: Option<usize>) -> bool {
        current_tab_id.is_some_and(|id| tabs.iter().any(|tab| tab.active && tab.tab_id == id))
    }

    pub(crate) fn current_tab_is_active(&self) -> bool {
        let current_tab_id = self.current_tab_id();
        self.known_active_tab_id
            .map(|active_tab_id| current_tab_id == Some(active_tab_id))
            .unwrap_or_else(|| Self::current_tab_is_active_in(&self.all_tabs, current_tab_id))
    }

    pub fn current_tab_id(&self) -> Option<usize> {
        self.current_tab.as_ref().map(|t| t.tab_id)
    }

    fn push_became_active_events(&self, events: &mut Vec<StateEvent>) {
        events.push(StateEvent::BecameActive);
        if let Some(focused_pane) = self
            .current_tab
            .as_ref()
            .and_then(CurrentTab::focused_pane_needing_attention)
        {
            events.push(StateEvent::FocusMoved {
                new_pane: Some(focused_pane),
            });
        }
    }

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
                    label: pane
                        .terminal_command
                        .as_deref()
                        .and_then(parse_running_command)
                        .map(FocusedPaneLabel::TerminalCommand)
                        .or_else(|| focused_pane_title_label(pane).map(FocusedPaneLabel::Title)),
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
        events.extend(self.agent_events_from_manifest(current_tab, new_focused_pane.as_ref(), panes, &new_pane_ids));

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
        let was_active = Self::current_tab_is_active_in(prev_tabs, self.current_tab_id());
        let is_active = Self::current_tab_is_active_in(new_tabs, self.current_tab_id());
        if !was_active && is_active {
            self.push_became_active_events(&mut events);
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
            .and_then(Cmd::tracked_agent)
            .is_some_and(|current_agent| event.agent.priority() < current_agent.priority())
        {
            return vec![];
        }

        let pane_id = event.pane_id;
        let agent = event.agent;
        let waiting_cmd = Cmd::waiting(
            agent,
            self.current_tab_is_active() && pane_is_focused(ct.focused_pane.as_ref(), pane_id),
        );
        match event.kind {
            AgentEventKind::Start => {
                if current == Some(&waiting_cmd) {
                    return vec![];
                }
                vec![StateEvent::AgentDetected { pane_id, agent }]
            }
            AgentEventKind::Busy => {
                if matches!(
                    current,
                    Some(Cmd::Agent {
                        agent: current_agent,
                        state: AgentState::Busy,
                    }) if *current_agent == agent
                ) {
                    return vec![];
                }
                vec![StateEvent::AgentBusy { pane_id, agent }]
            }
            AgentEventKind::Idle => {
                if current == Some(&waiting_cmd) {
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

    pub fn events_from_active_tab(&self, active_tab_id: usize) -> Vec<StateEvent> {
        if self.known_active_tab_id == Some(active_tab_id) {
            return vec![];
        }
        let mut events = vec![StateEvent::ActiveTabChanged { active_tab_id }];
        let was_active = self.current_tab_is_active();
        let is_active = self.current_tab_id() == Some(active_tab_id);
        if !was_active && is_active {
            self.push_became_active_events(&mut events);
        }
        events
    }

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
                    ct.busy_seq_by_pane.retain(|id, _| new_pane_ids.contains(id));
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
                    if let Some(pane) = new_pane.as_ref() {
                        ct.note_focused_agent_pane(pane.id);
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
                let is_active = self.current_tab_is_active();
                if let Some(ct) = self.current_tab.as_mut() {
                    let is_focused = is_active && pane_is_focused(ct.focused_pane.as_ref(), *pane_id);
                    ct.agent_by_pane.insert(*pane_id, Cmd::waiting(*agent, is_focused));
                    if is_focused {
                        ct.note_focused_agent_pane(*pane_id);
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::AgentBusy { pane_id, agent } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.agent_by_pane.insert(*pane_id, Cmd::agent(*agent, AgentState::Busy));
                    ct.busy_seq_by_pane.insert(*pane_id, ct.seq.saturating_add(1));
                    if ct.focused_pane.as_ref().map(|pane| pane.id) == Some(*pane_id) {
                        ct.note_focused_agent_pane(*pane_id);
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::AgentIdle { pane_id, agent } => {
                let is_active = self.current_tab_is_active();
                if let Some(ct) = self.current_tab.as_mut() {
                    let is_focused = is_active && pane_is_focused(ct.focused_pane.as_ref(), *pane_id);
                    ct.agent_by_pane.insert(*pane_id, Cmd::waiting(*agent, is_focused));
                    ct.busy_seq_by_pane.remove(pane_id);
                    if is_focused {
                        ct.note_focused_agent_pane(*pane_id);
                    }
                    ct.seq = ct.seq.saturating_add(1);
                }
            }
            StateEvent::AgentLost { pane_id } => {
                if let Some(ct) = self.current_tab.as_mut() {
                    ct.agent_by_pane.remove(pane_id);
                    ct.busy_seq_by_pane.remove(pane_id);
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
                self.known_active_tab_id = new_tabs.iter().find(|tab| tab.active).map(|tab| tab.tab_id);
                self.all_tabs = new_tabs.clone();
            }
            StateEvent::SyncRequested => {
                self.sync_requested = true;
            }
            StateEvent::ActiveTabChanged { active_tab_id } => {
                self.known_active_tab_id = Some(*active_tab_id);
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
        new_focused_pane: Option<&FocusedPane>,
        panes: &[PaneInfo],
        surviving_pane_ids: &HashSet<u32>,
    ) -> Vec<StateEvent> {
        let mut events = vec![];

        // Detect new agent in the newly-focused pane (persist across focus moves).
        if let Some(focused) = new_focused_pane
            && let Some(agent) = panes
                .iter()
                .find(|pane| pane.id == focused.id && !pane.is_plugin)
                .and_then(|pane| pane.terminal_command.as_deref())
                .and_then(parse_running_command)
                .and_then(|cmd| Agent::detect(&cmd))
        {
            match current_tab.agent_by_pane.get(&focused.id) {
                Some(cmd) if cmd.agent_name() == Some(agent.name()) => {}
                _ => {
                    events.push(StateEvent::AgentDetected {
                        pane_id: focused.id,
                        agent,
                    });
                }
            }
        }

        // Reconcile focused pane against manifest command metadata.
        for (&pane_id, cmd) in &current_tab.agent_by_pane {
            if !surviving_pane_ids.contains(&pane_id) {
                continue; // already emitted AgentLost in events_from_pane_update
            }
            let Some(stored_agent) = cmd.tracked_agent() else {
                continue;
            };
            let Some(pane) = panes.iter().find(|p| p.id == pane_id && !p.is_plugin) else {
                continue;
            };
            if pane.exited || pane.is_held || !pane.is_focused {
                continue; // only reconcile focused panes
            }
            let detected = pane
                .terminal_command
                .as_deref()
                .and_then(parse_running_command)
                .and_then(|exe| Agent::detect(&exe));
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

#[cfg_attr(test, derive(Debug, PartialEq))]
#[derive(Default)]
pub struct CurrentTab {
    pub tab_id: usize,
    pub seq: u64,
    pub pane_ids: HashSet<u32>,
    pub focused_pane: Option<FocusedPane>,
    pub last_focused_agent_pane_id: Option<u32>,
    pub busy_seq_by_pane: HashMap<u32, u64>,
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

    fn focused_pane_cmd(&self) -> Option<&Cmd> {
        let focused_pane = self.focused_pane.as_ref()?;
        self.agent_by_pane.get(&focused_pane.id)
    }

    fn focused_pane_needing_attention(&self) -> Option<FocusedPane> {
        let focused_pane = self.focused_pane.as_ref()?;
        self.focused_pane_cmd()
            .is_some_and(Cmd::needs_attention)
            .then(|| focused_pane.clone())
    }

    fn note_focused_agent_pane(&mut self, pane_id: u32) {
        let Some(cmd) = self.agent_by_pane.get_mut(&pane_id) else {
            return;
        };
        let _ = cmd.acknowledge();
        self.last_focused_agent_pane_id = Some(pane_id);
    }

    pub fn cmd(&self) -> Cmd {
        if let Some((pane_id, _)) = self.busy_seq_by_pane.iter().max_by_key(|(_, seq)| *seq)
            && let Some(
                cmd @ Cmd::Agent {
                    state: AgentState::Busy,
                    ..
                },
            ) = self.agent_by_pane.get(pane_id)
        {
            return cmd.clone();
        }

        if let Some(cmd @ Cmd::Agent { .. }) = self.focused_pane_cmd() {
            return cmd.clone();
        }

        if let Some(agent) = self
            .focused_pane
            .as_ref()
            .and_then(|focused| match focused.label.as_ref() {
                Some(FocusedPaneLabel::TerminalCommand(cmd)) => Some(cmd.as_str()),
                Some(FocusedPaneLabel::Title(_)) | None => None,
            })
            .and_then(Agent::detect)
        {
            return Cmd::agent(agent, AgentState::Acknowledged);
        }

        if self.agent_by_pane.len() == 1
            && let Some(cmd @ Cmd::Agent { .. }) = self.agent_by_pane.values().next()
        {
            return cmd.clone();
        }

        self.focused_pane
            .as_ref()
            .and_then(|focused| match focused.label.as_ref() {
                Some(FocusedPaneLabel::TerminalCommand(cmd)) | Some(FocusedPaneLabel::Title(cmd)) => {
                    Some(Cmd::Running(cmd.to_string()))
                }
                None => None,
            })
            .unwrap_or(Cmd::None)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FocusedPane {
    pub id: u32,
    pub label: Option<FocusedPaneLabel>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FocusedPaneLabel {
    TerminalCommand(String),
    Title(String),
}

fn pane_is_focused(focused_pane: Option<&FocusedPane>, pane_id: u32) -> bool {
    focused_pane.map(|pane| pane.id) == Some(pane_id)
}

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

fn focused_pane_title_label(pane: &PaneInfo) -> Option<String> {
    if pane.exited || pane.is_held {
        return None;
    }
    let title = pane.title.trim();
    (!title.is_empty()
        && !title.starts_with('~')
        && !title.starts_with('/')
        && title != "Pane"
        && !title.starts_with("Pane "))
    .then(|| ytil_tui::display_fixed_width(title, 8))
}

fn parse_running_command(command: &str) -> Option<String> {
    let executable = command.split_whitespace().next()?;
    let executable = executable.rsplit('/').next().unwrap_or(executable);
    if executable.is_empty() || matches!(executable, "zsh" | "bash" | "fish") {
        return None;
    }
    Some(executable.to_string())
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
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Codex, AgentState::Busy));

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
                    new_pane: Some(FocusedPane { id: 42, label: None }),
                },
                StateEvent::SyncRequested,
            ]
        );

        let current_tab = state.current_tab.as_ref().unwrap();
        let expected = CurrentTab {
            tab_id: 10,
            seq: 2, // bumped for PanesChanged and again for FocusMoved
            pane_ids: [42].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 42, label: None }),
            last_focused_agent_pane_id: Some(42),
            busy_seq_by_pane: HashMap::new(),
            cwd: Some(PathBuf::from("/tmp/project")),
            agent_by_pane: [(42, Cmd::agent(Agent::Codex, AgentState::Busy))].into_iter().collect(),
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
        ct.focused_pane = Some(FocusedPane { id: 42, label: None });
        ct.cwd = Some(PathBuf::from("/tmp/project"));
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Codex, AgentState::Busy));

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
            focused_pane: Some(FocusedPane { id: 42, label: None }),
            last_focused_agent_pane_id: None,
            busy_seq_by_pane: HashMap::new(),
            cwd: Some(PathBuf::from("/tmp/project")),
            agent_by_pane: [(42, Cmd::agent(Agent::Codex, AgentState::Busy))].into_iter().collect(),
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

        let snap1 = snapshot(2, 10, Cmd::agent(Agent::Codex, AgentState::Busy));
        let events1 = state.events_from_state_snapshot(100, &snap1);
        state.apply_all(&events1);

        let snap2 = snapshot(2, 1, Cmd::agent(Agent::Claude, AgentState::Acknowledged));
        let events2 = state.events_from_state_snapshot(200, &snap2);
        state.apply_all(&events2);

        let expected = HashMap::from([(200, snapshot(2, 1, Cmd::agent(Agent::Claude, AgentState::Acknowledged)))]);
        pretty_assertions::assert_eq!(state.other_tabs, expected);
    }

    #[rstest]
    #[case::fallback_to_focused_non_agent(
        vec![],
        Some((0, "cargo")),
        vec![],
        Cmd::Running("cargo".to_string())
    )]
    #[case::single_busy_agent_wins(
        vec![(42, Cmd::agent(Agent::Codex, AgentState::Busy))],
        Some((0, "cargo")),
        vec![(42, 1)],
        Cmd::agent(Agent::Codex, AgentState::Busy)
    )]
    #[case::last_busy_agent_wins(
        vec![(42, Cmd::agent(Agent::Claude, AgentState::Busy)), (43, Cmd::agent(Agent::Cursor, AgentState::Busy))],
        Some((42, "claude")),
        vec![(42, 1), (43, 2)],
        Cmd::agent(Agent::Cursor, AgentState::Busy)
    )]
    #[case::single_idle_agent_wins_over_focus(
        vec![(42, Cmd::agent(Agent::Claude, AgentState::Acknowledged))],
        Some((43, "cargo")),
        vec![],
        Cmd::agent(Agent::Claude, AgentState::Acknowledged)
    )]
    #[case::focused_idle_agent_wins_when_no_busy_agents(
        vec![(42, Cmd::agent(Agent::Claude, AgentState::Acknowledged))],
        Some((42, "claude")),
        vec![],
        Cmd::agent(Agent::Claude, AgentState::Acknowledged)
    )]
    #[case::focused_untracked_agent_wins_over_lone_tracked_agent(
        vec![(42, Cmd::agent(Agent::Claude, AgentState::Acknowledged))],
        Some((43, "cursor-agent")),
        vec![],
        Cmd::agent(Agent::Cursor, AgentState::Acknowledged)
    )]
    #[case::idle_agents_do_not_override_focus(
        vec![
            (42, Cmd::agent(Agent::Claude, AgentState::Acknowledged)),
            (43, Cmd::agent(Agent::Cursor, AgentState::Acknowledged)),
        ],
        Some((44, "cargo")),
        vec![],
        Cmd::Running("cargo".to_string())
    )]
    #[case::focused_agent_command_shows_idle_agent(
        vec![],
        Some((0, "/opt/homebrew/bin/codex")),
        vec![],
        Cmd::agent(Agent::Codex, AgentState::Acknowledged)
    )]
    fn test_current_tab_cmd(
        #[case] agents: Vec<(u32, Cmd)>,
        #[case] focused: Option<(u32, &str)>,
        #[case] busy_seq: Vec<(u32, u64)>,
        #[case] expected: Cmd,
    ) {
        let tab = CurrentTab {
            agent_by_pane: agents.into_iter().collect(),
            focused_pane: focused.map(|(id, cmd)| FocusedPane {
                id,
                label: Some(FocusedPaneLabel::TerminalCommand(cmd.to_string())),
            }),
            busy_seq_by_pane: busy_seq.into_iter().collect(),
            ..Default::default()
        };

        pretty_assertions::assert_eq!(tab.cmd(), expected);
    }

    #[test]
    fn test_agent_idle_in_unfocused_pane_sets_waiting_unseen_until_focus_moves_to_it() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.extend([42, 43]);
        ct.focused_pane = Some(FocusedPane {
            id: 43,
            label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
        });
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Codex, AgentState::Busy));
        ct.busy_seq_by_pane.insert(42, 1);

        let idle_events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        });
        pretty_assertions::assert_eq!(
            idle_events,
            vec![StateEvent::AgentIdle {
                pane_id: 42,
                agent: Agent::Codex,
            }]
        );

        state.apply_all(&idle_events);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Codex, AgentState::NeedsAttention))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Codex, AgentState::NeedsAttention));

        state.apply_all(&[StateEvent::FocusMoved {
            new_pane: Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
            }),
        }]);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Codex, AgentState::Acknowledged))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Codex, AgentState::Acknowledged));
    }

    #[test]
    fn test_agent_start_in_unfocused_pane_resets_seen_waiting_state_to_unseen() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.extend([42, 43]);
        ct.focused_pane = Some(FocusedPane {
            id: 43,
            label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
        });
        ct.agent_by_pane
            .insert(42, Cmd::agent(Agent::Codex, AgentState::Acknowledged));

        let events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Start,
        });
        pretty_assertions::assert_eq!(
            events,
            vec![StateEvent::AgentDetected {
                pane_id: 42,
                agent: Agent::Codex,
            }]
        );

        state.apply_all(&events);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Codex, AgentState::NeedsAttention))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Codex, AgentState::NeedsAttention));
    }

    #[test]
    fn test_agent_idle_in_inactive_tab_stays_waiting_unseen_even_if_stored_pane_is_focused() {
        let mut state = State {
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                active: false,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
        });
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Claude, AgentState::Busy));
        ct.busy_seq_by_pane.insert(42, 1);

        let events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Claude,
            kind: AgentEventKind::Idle,
        });
        pretty_assertions::assert_eq!(
            events,
            vec![StateEvent::AgentIdle {
                pane_id: 42,
                agent: Agent::Claude,
            }]
        );

        state.apply_all(&events);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Claude, AgentState::NeedsAttention))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Claude, AgentState::NeedsAttention));
    }

    #[test]
    fn test_tab_update_activation_promotes_waiting_unseen_in_focused_pane_to_waiting_seen() {
        let mut state = State {
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                active: false,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
        });
        ct.agent_by_pane
            .insert(42, Cmd::agent(Agent::Claude, AgentState::NeedsAttention));

        let mut new_tabs = vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }];
        let events = state.events_from_tab_update(&mut new_tabs);
        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::AllTabsReplaced {
                    new_tabs: new_tabs.clone(),
                },
                StateEvent::BecameActive,
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                    }),
                },
                StateEvent::SyncRequested,
            ]
        );

        state.apply_all(&events);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Claude, AgentState::Acknowledged))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Claude, AgentState::Acknowledged));
    }

    #[test]
    fn test_active_tab_sync_overrides_stale_local_active_state_before_agent_idle() {
        let mut state = State {
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
        });
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Claude, AgentState::Busy));
        ct.busy_seq_by_pane.insert(42, 1);

        let active_tab_events = state.events_from_active_tab(20);
        pretty_assertions::assert_eq!(
            active_tab_events,
            vec![StateEvent::ActiveTabChanged { active_tab_id: 20 }]
        );
        state.apply_all(&active_tab_events);

        let idle_events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Claude,
            kind: AgentEventKind::Idle,
        });
        pretty_assertions::assert_eq!(
            idle_events,
            vec![StateEvent::AgentIdle {
                pane_id: 42,
                agent: Agent::Claude,
            }]
        );
        state.apply_all(&idle_events);

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Claude, AgentState::NeedsAttention))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Claude, AgentState::NeedsAttention));
    }

    #[test]
    fn test_active_tab_sync_promotes_waiting_unseen_in_focused_pane_when_tab_becomes_active_again() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                active: false,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
        });
        ct.agent_by_pane
            .insert(42, Cmd::agent(Agent::Claude, AgentState::NeedsAttention));

        let events = state.events_from_active_tab(10);
        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::ActiveTabChanged { active_tab_id: 10 },
                StateEvent::BecameActive,
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                    }),
                },
            ]
        );

        state.apply_all(&events);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Claude, AgentState::Acknowledged))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Claude, AgentState::Acknowledged));
    }

    #[test]
    fn test_remote_snapshot_preserves_waiting_unseen_state() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(1)),
            all_tabs: vec![tab_with_name(1, 0, "local"), tab_with_name(2, 1, "remote")],
            ..Default::default()
        };

        let snapshot = snapshot(2, 10, Cmd::agent(Agent::Codex, AgentState::NeedsAttention));
        let events = state.events_from_state_snapshot(100, &snapshot);
        state.apply_all(&events);

        pretty_assertions::assert_eq!(
            state.remote_snapshot_for_tab(2).map(|remote| remote.cmd.clone()),
            Some(snapshot.cmd)
        );
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_picks_focused_command_name() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                name: "a".to_string(),
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane { id: 42, label: None });
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
                        label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string()))
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
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                name: "a".to_string(),
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane { id: 42, label: None });
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
                        label: Some(FocusedPaneLabel::Title("nvim".to_string()))
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
            label: Some(FocusedPaneLabel::Title("nvim".to_string())),
        });
        ct.cwd = Some(PathBuf::from("/tmp/project"));

        let pane = terminal_pane_with_title(42, true, "/tmp/project");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane { id: 42, label: None })
                },
                StateEvent::SyncRequested,
            ]
        );
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.focused_pane, Some(FocusedPane { id: 42, label: None }));
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::None);
    }

    #[test]
    fn test_refresh_current_tab_uses_title_when_command_missing() {
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
            label: Some(FocusedPaneLabel::Title("nv".to_string())),
        });
        ct.cwd = Some(PathBuf::from("/tmp/project"));

        let pane = terminal_pane_with_title(42, true, "CLAUDE.md");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::Title("CLAUDE.…".to_string()))
                    })
                },
                StateEvent::SyncRequested
            ]
        );
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.focused_pane,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::Title("CLAUDE.…".to_string()))
            })
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::Running("CLAUDE.…".to_string()));
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
            label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
        });
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Codex, AgentState::Busy));

        let pane = terminal_pane_with_command(42, true, "/bin/zsh");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane { id: 42, label: None })
                },
                // Only one AgentLost: the detection-check is skipped for busy agent state (the agent
                // explicitly set itself busy via hook, so trust it over title changes), but the
                // reconcile loop still clears it because terminal_command changed to zsh.
                StateEvent::AgentLost { pane_id: 42 },
                StateEvent::SyncRequested,
            ]
        );

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.agent_by_pane.get(&42), None);
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::None);
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_keeps_idle_agent_when_focused_pane_title_becomes_path() {
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
            label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
        });
        ct.agent_by_pane
            .insert(42, Cmd::agent(Agent::Codex, AgentState::Acknowledged));

        let pane = terminal_pane_with_title(42, true, "/tmp/project");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane { id: 42, label: None })
                },
                StateEvent::SyncRequested,
            ]
        );

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Codex, AgentState::Acknowledged))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Codex, AgentState::Acknowledged));
    }

    #[test]
    fn test_frame_keeps_agent_indicator_when_focused_pane_title_changes_to_cwd() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                name: "agent".to_string(),
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab::new(10)),
            home_dir: PathBuf::from("/home/user"),
            ..Default::default()
        };

        let ct = state.current_tab.as_mut().unwrap();
        ct.pane_ids.insert(42);
        ct.focused_pane = Some(FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
        });
        ct.cwd = Some(PathBuf::from("/home/user/project"));
        ct.agent_by_pane
            .insert(42, Cmd::agent(Agent::Codex, AgentState::Acknowledged));

        let pane = terminal_pane_with_title(42, true, "/home/user/project");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane { id: 42, label: None })
                },
                StateEvent::SyncRequested,
            ]
        );

        pretty_assertions::assert_eq!(
            state.frame,
            vec![TabRow {
                active: true,
                path_label: "~/project".to_string(),
                cmd: Cmd::agent(Agent::Codex, AgentState::Acknowledged),
                git: GitStat::default(),
            }]
        );
    }

    #[test]
    fn test_busy_agent_not_cleared_when_pane_title_changes_to_session_name() {
        // Regression: Cursor (and similar agents) change the pane title to the session name while
        // processing. This must not clear busy agent state set by an explicit hook event.
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
            label: Some(FocusedPaneLabel::TerminalCommand("cursor".to_string())),
        });
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Cursor, AgentState::Busy));
        ct.busy_seq_by_pane.insert(42, 1);

        // Cursor changed pane title to session name; terminal_command is absent.
        let pane = terminal_pane_with_title(42, true, "my-project-session");
        let m = manifest(vec![(0, vec![plugin_pane(7), pane])]);
        let events = apply_pane_update(&mut state, &m);

        pretty_assertions::assert_eq!(
            events,
            vec![
                StateEvent::FocusMoved {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::Title("my-proj…".to_string())),
                    })
                },
                StateEvent::SyncRequested,
            ]
        );

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Cursor, AgentState::Busy))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Cursor, AgentState::Busy));
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
            label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
        });
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Codex, AgentState::Busy));
        ct.busy_seq_by_pane.insert(42, 1);
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
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Codex, AgentState::Busy))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Codex, AgentState::Busy));
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_persists_agent_seen_in_focused_pane() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                name: "a".to_string(),
                active: true,
                ..Default::default()
            }],
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
                        label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string()))
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
                        label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string()))
                    })
                },
            ]
        );

        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(
            ct.agent_by_pane.get(&42),
            Some(&Cmd::agent(Agent::Codex, AgentState::Acknowledged))
        );
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Codex, AgentState::Acknowledged));
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_prefers_last_busy_agent_when_multiple_agents_exist() {
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
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Cursor, AgentState::Busy));

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
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Cursor, AgentState::Busy));
    }

    #[test]
    fn test_refresh_current_tab_from_manifest_multiple_idle_agents_follow_focus() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![TabInfo {
                tab_id: 10,
                position: 0,
                name: "a".to_string(),
                active: true,
                ..Default::default()
            }],
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
        state.apply_all(&state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Claude,
            kind: AgentEventKind::Idle,
        }));
        state.apply_all(&state.events_from_agent_event(&AgentEventPayload {
            pane_id: 43,
            agent: Agent::Cursor,
            kind: AgentEventKind::Idle,
        }));

        let second = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, false, "/opt/homebrew/bin/claude"),
                terminal_pane_with_command(43, true, "/usr/bin/cargo test"),
            ],
        )]);
        apply_pane_update(&mut state, &second);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Claude, AgentState::Acknowledged));
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
            label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
        });
        ct.last_focused_agent_pane_id = Some(42);
        ct.agent_by_pane.insert(42, Cmd::agent(Agent::Claude, AgentState::Busy));
        ct.agent_by_pane.insert(43, Cmd::agent(Agent::Cursor, AgentState::Busy));
        ct.busy_seq_by_pane.insert(42, 1);
        ct.busy_seq_by_pane.insert(43, 2);

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
                        label: Some(FocusedPaneLabel::TerminalCommand("cursor-agent".to_string()))
                    })
                },
                StateEvent::SyncRequested,
            ]
        );

        state.apply_all(&events);
        let ct = state.current_tab.as_ref().unwrap();
        pretty_assertions::assert_eq!(ct.seq, 6);
        pretty_assertions::assert_eq!(ct.last_focused_agent_pane_id, Some(43));
        pretty_assertions::assert_eq!(ct.cmd(), Cmd::agent(Agent::Cursor, AgentState::Busy));
    }

    #[rstest]
    #[case("zsh", Some("zsh".to_string()))]
    #[case("bash", Some("bash".to_string()))]
    #[case("fish", Some("fish".to_string()))]
    #[case("cargo test", Some("cargo t…".to_string()))]
    #[case("✳ Claude Code", Some("✳ Claud…".to_string()))]
    #[case("Cursor Agent", Some("Cursor …".to_string()))]
    #[case("CLAUDE.md", Some("CLAUDE.…".to_string()))]
    #[case("CLAUDE.md [+]", Some("CLAUDE.…".to_string()))]
    #[case(".claude/CLAUDE.md", Some(".claude…".to_string()))]
    #[case("averylongtitle", Some("averylo…".to_string()))]
    fn test_focused_pane_label(#[case] input: &str, #[case] expected: Option<String>) {
        let pane = terminal_pane_with_title(42, true, input);
        pretty_assertions::assert_eq!(focused_pane_title_label(&pane), expected);
    }
}
