use std::collections::HashSet;
use std::path::PathBuf;

use agg::GitStat;
use ytil_agents::agent::AgentEventKind;
use ytil_agents::agent::AgentEventPayload;
use zellij_tile::prelude::*;

use super::State;
use super::current_tab::AgentPanePhase;
use super::current_tab::CurrentTab;
use super::current_tab::FocusedPane;
use super::current_tab::idle_phase_for_pane;
use super::pane::detected_agent_from_pane_info;
use super::pane::focused_pane_from_pane_info;
use super::tabs::detect_remapped_tab_id;
use super::tabs::topology_changed;
use crate::wasm::events::StateEvent;
use crate::wasm::plugin::StateSnapshotPayload;

impl State {
    pub(crate) fn current_tab_is_active(&self) -> bool {
        let current_tab_id = self.current_tab_id();
        self.known_active_tab_id.map_or_else(
            || Self::current_tab_is_active_in(&self.all_tabs, current_tab_id),
            |active_tab_id| current_tab_id == Some(active_tab_id),
        )
    }

    pub fn current_tab_id(&self) -> Option<usize> {
        self.current_tab.as_ref().map(|current_tab| current_tab.tab_id)
    }

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
        let current_tab_id = self.current_tab.as_ref().map(|current_tab| current_tab.tab_id);
        let discovered_tab_id = self
            .all_tabs
            .iter()
            .find(|tab| tab.position == tab_pos)
            .map(|tab| tab.tab_id);

        let bootstrapped_current_tab =
            Self::bootstrap_current_tab_for_pane_update(self.current_tab.as_ref(), discovered_tab_id);
        if let Some(current_tab) = bootstrapped_current_tab.as_ref() {
            events.push(StateEvent::TabCreated {
                tab_id: current_tab.tab_id,
            });
        }

        if let (Some(current_id), Some(discovered_id)) = (current_tab_id, discovered_tab_id)
            && !self.all_tabs.iter().any(|tab| tab.tab_id == current_id)
        {
            events.push(StateEvent::TabRemapped {
                new_tab_id: discovered_id,
            });
        }

        let Some(current_tab) = self.current_tab.as_ref().or(bootstrapped_current_tab.as_ref()) else {
            return events;
        };

        let mut new_pane_ids = HashSet::new();
        let mut new_focused_pane = None;
        for pane in panes
            .iter()
            .filter(|pane| !pane.is_plugin && !pane.exited && !pane.is_held)
        {
            new_pane_ids.insert(pane.id);
            if pane.is_focused {
                new_focused_pane = focused_pane_from_pane_info(pane);
            }
        }

        if new_pane_ids != current_tab.pane_ids {
            let observed_pane_ids = new_pane_ids.clone();
            let mut retained_pane_ids = observed_pane_ids.clone();
            for removed_pane_id in current_tab.pane_ids.difference(&observed_pane_ids) {
                if !current_tab.pane_state_by_pane.contains_key(removed_pane_id) {
                    continue;
                }
                if current_tab
                    .missed_pane_updates_by_pane
                    .get(removed_pane_id)
                    .copied()
                    .unwrap_or(0)
                    == 0
                {
                    retained_pane_ids.insert(*removed_pane_id);
                } else {
                    events.push(StateEvent::AgentLost {
                        pane_id: *removed_pane_id,
                    });
                }
            }
            events.push(StateEvent::PanesChanged {
                observed_pane_ids,
                retained_pane_ids,
            });
        }

        let new_focus_pane_id = new_focused_pane.as_ref().map(|pane| pane.id);
        let focused_pane_id_changed = new_focus_pane_id != current_tab.focused_pane.as_ref().map(|pane| pane.id);
        let focused_metadata_changed = new_focused_pane != current_tab.focused_pane;
        let focus_tracking_changed =
            self.current_tab_is_active() && current_tab.active_focus_pane_id != new_focus_pane_id;
        let pending_activation_focus_ack =
            self.current_tab_is_active() && current_tab.pending_activation_focus_ack && new_focus_pane_id.is_some();
        if focused_metadata_changed || focus_tracking_changed || pending_activation_focus_ack {
            events.push(StateEvent::FocusChanged {
                new_pane: new_focused_pane.clone(),
                acknowledge_existing_attention: pending_activation_focus_ack
                    || self.current_tab_is_active() && focused_pane_id_changed && new_focus_pane_id.is_some(),
            });
        }

        events.extend(Self::agent_events_from_manifest(
            current_tab,
            new_focused_pane.as_ref(),
            panes,
            &new_pane_ids,
        ));

        if let Some(focused_pane) = new_focused_pane.as_ref()
            && (focused_metadata_changed || current_tab.cwd.is_none())
            && let Some(new_cwd) = resolve_pane_cwd(focused_pane.id)
            && current_tab.cwd.as_ref() != Some(&new_cwd)
        {
            events.push(StateEvent::CwdChanged { new_cwd });
        }

        self.push_pane_update_sync_event(&mut events);

        events
    }

    pub fn events_from_tab_update(
        &self,
        new_tabs: &mut [TabInfo],
        landing_focus: Option<FocusedPane>,
    ) -> Vec<StateEvent> {
        new_tabs.sort_by_key(|tab| tab.position);

        let prev_tabs = &self.all_tabs;
        let mut events = vec![StateEvent::AllTabsReplaced {
            new_tabs: new_tabs.to_vec(),
        }];

        if topology_changed(prev_tabs, new_tabs) {
            events.push(StateEvent::TopologyChanged);
        }

        let was_active = Self::current_tab_is_active_in(prev_tabs, self.current_tab_id());
        let is_active = Self::current_tab_is_active_in(new_tabs, self.current_tab_id());
        if !was_active && is_active {
            Self::push_became_active_events(&mut events, landing_focus);
        }

        let has_remap =
            detect_remapped_tab_id(self.current_tab.as_ref(), prev_tabs, new_tabs).is_some_and(|new_tab_id| {
                events.push(StateEvent::TabRemapped { new_tab_id });
                true
            });

        if self.current_tab.is_some() && (!self.sync_requested || topology_changed(prev_tabs, new_tabs) || has_remap) {
            events.push(StateEvent::SyncRequested);
        }

        events
    }

    pub fn events_from_cwd_changed(&self, pane_id: u32, cwd: PathBuf) -> Vec<StateEvent> {
        let Some(current_tab) = self.current_tab.as_ref() else {
            return vec![];
        };
        if current_tab.focused_pane.as_ref().map(|focused_pane| focused_pane.id) != Some(pane_id) {
            return vec![];
        }
        if current_tab.cwd.as_ref() == Some(&cwd) {
            return vec![];
        }
        vec![StateEvent::CwdChanged { new_cwd: cwd }]
    }

    pub fn events_from_pane_closed(&self, pane_id: u32) -> Vec<StateEvent> {
        let Some(current_tab) = self.current_tab.as_ref() else {
            return vec![];
        };
        if !current_tab.pane_state_by_pane.contains_key(&pane_id) {
            return vec![];
        }
        vec![StateEvent::AgentLost { pane_id }]
    }

    pub fn events_from_run_command_result(
        &self,
        requested_cwd: &PathBuf,
        exit_code: Option<i32>,
        stdout: &[u8],
    ) -> Vec<StateEvent> {
        if exit_code != Some(0) {
            return vec![];
        }
        let Some(current_tab) = self.current_tab.as_ref() else {
            return vec![];
        };
        if current_tab.cwd.as_ref() != Some(requested_cwd) {
            return vec![];
        }

        let output = String::from_utf8_lossy(stdout);
        for line in output.lines() {
            let Ok((path, new_stat)) = GitStat::parse_line(line).inspect_err(|error| eprintln!("agg: {error}")) else {
                continue;
            };
            if path != *requested_cwd {
                continue;
            }
            if current_tab.git_stat == new_stat {
                return vec![];
            }
            return vec![StateEvent::GitStatChanged { new_stat }];
        }

        vec![]
    }

    pub fn events_from_agent_event(&self, event: &AgentEventPayload) -> Vec<StateEvent> {
        let Some(current_tab) = self.current_tab.as_ref() else {
            return vec![];
        };
        if !current_tab.pane_ids.contains(&event.pane_id) {
            return vec![];
        }

        let current_pane_state = current_tab.pane_state_by_pane.get(&event.pane_id);
        if current_pane_state.is_some_and(|pane_state| event.agent.priority() < pane_state.agent.priority()) {
            return vec![];
        }

        let pane_id = event.pane_id;
        let agent = event.agent;
        let current_tab_is_active = self.current_tab_is_active();
        match event.kind {
            AgentEventKind::Start => {
                if current_pane_state.is_some_and(|pane_state| pane_state.agent == agent) {
                    return vec![];
                }
                vec![StateEvent::AgentDetected { pane_id, agent }]
            }
            AgentEventKind::Busy => {
                if current_pane_state
                    .is_some_and(|pane_state| pane_state.agent == agent && pane_state.phase == AgentPanePhase::Running)
                {
                    return vec![];
                }
                vec![StateEvent::AgentBusy { pane_id, agent }]
            }
            AgentEventKind::Idle => {
                let desired_phase = idle_phase_for_pane(current_tab, current_tab_is_active, pane_id);
                if current_pane_state
                    .is_some_and(|pane_state| pane_state.agent == agent && pane_state.phase == desired_phase)
                {
                    return vec![];
                }
                vec![StateEvent::AgentIdle { pane_id, agent }]
            }
            AgentEventKind::Exit => {
                if current_pane_state.is_none() {
                    return vec![];
                }
                vec![StateEvent::AgentLost { pane_id }]
            }
        }
    }

    pub fn events_from_state_snapshot(
        &self,
        source_plugin_id: u32,
        snapshot: &StateSnapshotPayload,
    ) -> Vec<StateEvent> {
        if source_plugin_id == self.plugin_id
            || self.current_tab_id() == Some(snapshot.tab_id)
            || !self.all_tabs.iter().any(|tab| tab.tab_id == snapshot.tab_id)
            || self
                .other_tabs
                .get(&source_plugin_id)
                .is_some_and(|remote| snapshot.seq <= remote.seq)
        {
            return vec![];
        }

        let evict_ids = self
            .other_tabs
            .iter()
            .filter(|&(plugin_id, remote)| *plugin_id != source_plugin_id && remote.tab_id == snapshot.tab_id)
            .map(|(&plugin_id, _)| plugin_id)
            .collect();

        vec![StateEvent::RemoteTabUpdated {
            source_plugin_id,
            snapshot: snapshot.clone(),
            evict_ids,
        }]
    }

    pub fn events_from_active_tab(&self, active_tab_id: usize, landing_focus: Option<FocusedPane>) -> Vec<StateEvent> {
        if self.known_active_tab_id == Some(active_tab_id) {
            if let Some(event) = self.same_tab_activation_focus_event(active_tab_id, landing_focus) {
                return vec![event];
            }
            return vec![];
        }

        let mut events = vec![StateEvent::ActiveTabChanged { active_tab_id }];
        let was_active = self.current_tab_is_active();
        let is_active = self.current_tab_id() == Some(active_tab_id);
        if !was_active && is_active {
            Self::push_became_active_events(&mut events, landing_focus);
        }

        events
    }

    fn current_tab_is_active_in(tabs: &[TabInfo], current_tab_id: Option<usize>) -> bool {
        current_tab_id.is_some_and(|id| tabs.iter().any(|tab| tab.active && tab.tab_id == id))
    }

    fn push_became_active_events(events: &mut Vec<StateEvent>, landing_focus: Option<FocusedPane>) {
        events.push(StateEvent::BecameActive);
        if let Some(focused_pane) = landing_focus {
            events.push(StateEvent::FocusChanged {
                new_pane: Some(focused_pane),
                acknowledge_existing_attention: true,
            });
        }
    }

    fn same_tab_activation_focus_event(
        &self,
        active_tab_id: usize,
        landing_focus: Option<FocusedPane>,
    ) -> Option<StateEvent> {
        let current_tab = self.current_tab.as_ref()?;
        if current_tab.tab_id != active_tab_id {
            return None;
        }

        let landing_focus = landing_focus?;
        Self::should_reconcile_same_tab_activation_focus(current_tab, &landing_focus).then_some(
            StateEvent::FocusChanged {
                new_pane: Some(landing_focus),
                acknowledge_existing_attention: true,
            },
        )
    }

    fn should_reconcile_same_tab_activation_focus(current_tab: &CurrentTab, landing_focus: &FocusedPane) -> bool {
        let landing_focus_id = landing_focus.id;
        current_tab.pending_activation_focus_ack
            || current_tab.active_focus_pane_id != Some(landing_focus_id)
            || current_tab.focused_pane.as_ref().map(|pane| pane.id) != Some(landing_focus_id)
            || current_tab
                .pane_state_by_pane
                .get(&landing_focus_id)
                .is_some_and(|pane_state| pane_state.phase == AgentPanePhase::AttentionUnseen)
    }

    fn push_pane_update_sync_event(&self, events: &mut Vec<StateEvent>) {
        let has_resetter = events
            .iter()
            .any(|event| matches!(event, StateEvent::TabCreated { .. } | StateEvent::TabRemapped { .. }));
        if has_resetter || self.current_tab.is_some() && !self.sync_requested {
            events.push(StateEvent::SyncRequested);
        }
    }

    fn current_tab_position_in_manifest(&self, manifest: &PaneManifest) -> Option<usize> {
        manifest.panes.iter().find_map(|(tab_pos, panes)| {
            panes
                .iter()
                .any(|pane| pane.is_plugin && pane.id == self.plugin_id)
                .then_some(*tab_pos)
        })
    }

    fn bootstrap_current_tab_for_pane_update(
        current_tab: Option<&CurrentTab>,
        discovered_tab_id: Option<usize>,
    ) -> Option<CurrentTab> {
        if current_tab.is_some() {
            return None;
        }
        let tab_id = discovered_tab_id?;
        Some(CurrentTab::new(tab_id))
    }

    fn agent_events_from_manifest(
        current_tab: &CurrentTab,
        new_focused_pane: Option<&FocusedPane>,
        panes: &[PaneInfo],
        surviving_pane_ids: &HashSet<u32>,
    ) -> Vec<StateEvent> {
        let mut events = vec![];
        let Some(focused_pane) = new_focused_pane else {
            return events;
        };
        let Some(pane) = panes.iter().find(|pane| pane.id == focused_pane.id && !pane.is_plugin) else {
            return events;
        };
        if pane.exited || pane.is_held {
            return events;
        }

        let stored_agent = current_tab
            .pane_state_by_pane
            .get(&focused_pane.id)
            .map(|pane_state| pane_state.agent);
        let detected_agent = detected_agent_from_pane_info(pane, focused_pane);
        let has_terminal_command = pane
            .terminal_command
            .as_ref()
            .is_some_and(|command| !command.trim().is_empty());

        match (stored_agent, detected_agent) {
            (Some(stored_agent), Some(detected_agent)) if stored_agent != detected_agent => {
                events.push(StateEvent::AgentLost {
                    pane_id: focused_pane.id,
                });
                events.push(StateEvent::AgentDetected {
                    pane_id: focused_pane.id,
                    agent: detected_agent,
                });
            }
            (None, Some(detected_agent)) => {
                events.push(StateEvent::AgentDetected {
                    pane_id: focused_pane.id,
                    agent: detected_agent,
                });
            }
            (Some(_), None) if has_terminal_command => {
                events.push(StateEvent::AgentLost {
                    pane_id: focused_pane.id,
                });
            }
            _ => {}
        }

        for (&pane_id, pane_state) in &current_tab.pane_state_by_pane {
            if pane_id == focused_pane.id || !surviving_pane_ids.contains(&pane_id) {
                continue;
            }
            let Some(other_pane) = panes.iter().find(|pane| pane.id == pane_id && !pane.is_plugin) else {
                continue;
            };
            if other_pane.exited || other_pane.is_held {
                continue;
            }
            let detected_agent = focused_pane_from_pane_info(other_pane)
                .as_ref()
                .and_then(|focused_pane| detected_agent_from_pane_info(other_pane, focused_pane));
            let has_terminal_command = other_pane
                .terminal_command
                .as_ref()
                .is_some_and(|command| !command.trim().is_empty());
            if has_terminal_command && detected_agent != Some(pane_state.agent) {
                events.push(StateEvent::AgentLost { pane_id });
            }
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use agg::AgentState;
    use agg::Cmd;
    use agg::TabIndicator;
    use assert2::assert;
    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;

    use super::*;
    use crate::wasm::state::current_tab::FocusedPaneLabel;
    use crate::wasm::state::current_tab::PaneFocus;
    use crate::wasm::state::nudge::Nudge;
    use crate::wasm::state::test_support::*;

    #[test]
    fn test_events_from_agent_event_start_sets_empty_state() {
        let mut state = State {
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.pane_ids.insert(42);

        let events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Start,
        });
        assert_eq!(
            events,
            vec![StateEvent::AgentDetected {
                pane_id: 42,
                agent: Agent::Codex,
            }]
        );

        let _ = state.apply_all(&events);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_apply_pane_update_first_detected_agent_starts_empty_until_busy() {
        let mut state = State {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                active_focus_pane_id: Some(42),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![plugin_pane(7), terminal_pane_with_command(42, true, "claude")],
            )]),
        );
        assert_eq!(
            events,
            vec![
                StateEvent::AgentDetected {
                    pane_id: 42,
                    agent: Agent::Claude,
                },
                StateEvent::SyncRequested,
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Claude, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_apply_pane_update_bootstraps_current_tab_and_detects_codex_on_first_update() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![TabInfo {
                active: true,
                ..tab_with_name(10, 0, "a")
            }],
            ..Default::default()
        };

        let events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![plugin_pane(7), terminal_pane_with_command(42, true, "codex")],
            )]),
        );
        assert_eq!(
            events,
            vec![
                StateEvent::TabCreated { tab_id: 10 },
                StateEvent::PanesChanged {
                    observed_pane_ids: std::iter::once(42).collect(),
                    retained_pane_ids: std::iter::once(42).collect(),
                },
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                StateEvent::AgentDetected {
                    pane_id: 42,
                    agent: Agent::Codex,
                },
                StateEvent::SyncRequested,
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_apply_pane_update_bootstraps_current_tab_without_detecting_non_agent_command() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![TabInfo {
                active: true,
                ..tab_with_name(10, 0, "a")
            }],
            ..Default::default()
        };

        let events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![
                    plugin_pane(7),
                    terminal_pane_with_command(42, true, "/usr/bin/cargo test"),
                ],
            )]),
        );
        assert_eq!(
            events,
            vec![
                StateEvent::TabCreated { tab_id: 10 },
                StateEvent::PanesChanged {
                    observed_pane_ids: std::iter::once(42).collect(),
                    retained_pane_ids: std::iter::once(42).collect(),
                },
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                StateEvent::SyncRequested,
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(current_tab.pane_state_by_pane.is_empty());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::None);
        assert_eq!(current_tab.display_cmd(), Cmd::Running("cargo".to_string()));
    }

    #[test]
    fn test_apply_pane_update_bootstraps_current_tab_and_detects_codex_from_title_on_first_update() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![TabInfo {
                active: true,
                ..tab_with_name(10, 0, "a")
            }],
            ..Default::default()
        };

        let events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![plugin_pane(7), terminal_pane_with_title(42, true, "codex")],
            )]),
        );
        assert_eq!(
            events,
            vec![
                StateEvent::TabCreated { tab_id: 10 },
                StateEvent::PanesChanged {
                    observed_pane_ids: std::iter::once(42).collect(),
                    retained_pane_ids: std::iter::once(42).collect(),
                },
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::Title("codex".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                StateEvent::AgentDetected {
                    pane_id: 42,
                    agent: Agent::Codex,
                },
                StateEvent::SyncRequested,
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_agent_idle_in_unfocused_pane_transitions_green_to_red() {
        let mut state = State {
            all_tabs: vec![tab_with_name(10, 0, "fallback-tab")],
            current_tab: Some(CurrentTab::new(10)),
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };
        assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.cwd = Some(PathBuf::from("/Users/me/project"));
        current_tab.pane_ids.extend([42, 43]);
        current_tab.focused_pane = Some(FocusedPane {
            id: 43,
            label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
        });
        current_tab.active_focus_pane_id = Some(43);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
        );

        let events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        });
        assert_eq!(
            events,
            vec![StateEvent::AgentIdle {
                pane_id: 42,
                agent: Agent::Codex,
            }]
        );

        let _ = state.apply_all(&events);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::NeedsAttention)
        );

        let nudge = Nudge::new(current_tab, &state.all_tabs, &state.home_dir, 42);
        assert_eq!(
            nudge,
            Some(Nudge {
                agent: Agent::Codex,
                tab_id: 10,
                pane_id: 42,
                path: ytil_tui::short_path(&PathBuf::from("/Users/me/project"), &PathBuf::from("/Users/me")),
            })
        );
        let nudge = nudge.expect("nudge");
        assert_eq!(nudge.title(), "🔴 Codex done");
        assert_eq!(nudge.body(), "~/project · t10 p42");
        let nudges = state.nudges();
        assert!(let [(42, nudge)] = nudges.as_slice());
        assert_eq!(nudge.agent, Agent::Codex);
        assert_eq!(nudge.tab_id, 10);
        assert_eq!(nudge.pane_id, 42);
        assert_eq!(
            nudge.path,
            ytil_tui::short_path(&PathBuf::from("/Users/me/project"), &PathBuf::from("/Users/me"))
        );
        assert_eq!(nudge.title(), "🔴 Codex done");
        assert_eq!(nudge.body(), "~/project · t10 p42");
        assert!(!state.has_nudged(42));
        state.mark_nudged(42);
        assert!(state.has_nudged(42));
        assert!(state.nudges().is_empty());
    }

    #[test]
    fn test_agent_idle_in_focused_pane_transitions_green_to_empty() {
        let mut state = State {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        assert!(let Some(current_tab) = state.current_tab.as_mut());
        current_tab.pane_ids.insert(42);
        current_tab.focused_pane = Some(FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
        });
        current_tab.active_focus_pane_id = Some(42);
        current_tab.pane_state_by_pane.insert(
            42,
            pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Focused, 1),
        );

        let events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        });
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_agent_idle_in_inactive_tab_with_stale_focus_transitions_to_red() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "a"), tab_with_name(20, 1, "b")],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        });
        assert_eq!(
            events,
            vec![StateEvent::AgentIdle {
                pane_id: 42,
                agent: Agent::Codex,
            }]
        );

        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);
        assert_eq!(
            current_tab.current_row_display(false),
            (Cmd::agent(Agent::Codex, AgentState::NeedsAttention), TabIndicator::Red,)
        );
    }

    #[test]
    fn test_partial_manifest_does_not_drop_running_agent_on_first_miss() {
        let mut state = State {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                active_focus_pane_id: Some(43),
                pane_state_by_pane: HashMap::from([
                    (
                        42,
                        pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                    ),
                    (
                        43,
                        pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Focused, 2),
                    ),
                ]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let partial_events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![plugin_pane(7), terminal_pane_with_command(43, true, "claude")],
            )]),
        );
        assert_eq!(
            partial_events,
            vec![
                StateEvent::PanesChanged {
                    observed_pane_ids: std::iter::once(43).collect(),
                    retained_pane_ids: [42, 43].into_iter().collect(),
                },
                StateEvent::SyncRequested,
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.pane_ids, HashSet::from([42, 43]));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Green);
        assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Codex, AgentState::Busy));

        state.sync_frame();
        let frame = &state.frame;
        assert!(let Some(row) = frame.first());
        assert_eq!(row.cmd, Cmd::agent(Agent::Claude, AgentState::Busy));
        assert_eq!(row.indicator, TabIndicator::Green);
    }

    #[test]
    fn test_partial_manifest_drops_missing_running_agent_after_second_miss() {
        let mut state = State {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                active_focus_pane_id: Some(43),
                pane_state_by_pane: HashMap::from([
                    (
                        42,
                        pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                    ),
                    (
                        43,
                        pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Focused, 2),
                    ),
                ]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let partial_manifest = manifest(vec![(
            0,
            vec![plugin_pane(7), terminal_pane_with_command(43, true, "claude")],
        )]);
        let _ = apply_pane_update(&mut state, &partial_manifest);
        let partial_events = apply_pane_update(&mut state, &partial_manifest);
        assert_eq!(
            partial_events,
            vec![
                StateEvent::AgentLost { pane_id: 42 },
                StateEvent::PanesChanged {
                    observed_pane_ids: std::iter::once(43).collect(),
                    retained_pane_ids: std::iter::once(43).collect(),
                },
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.pane_ids, HashSet::from([43]));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Green);
        assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Claude, AgentState::Busy));
    }

    #[test]
    fn test_mat_requires_each_pane_focus_to_clear_red() {
        let mut state = State {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                pane_state_by_pane: HashMap::from([
                    (
                        42,
                        pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                    ),
                    (
                        43,
                        pane_state(Agent::Cursor, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 2),
                    ),
                ]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events_a = vec![StateEvent::FocusChanged {
            new_pane: Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
            acknowledge_existing_attention: true,
        }];
        let _ = state.apply_all(&events_a);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);

        let events_b = vec![StateEvent::FocusChanged {
            new_pane: Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("cursor".to_string())),
            }),
            acknowledge_existing_attention: true,
        }];
        let _ = state.apply_all(&events_b);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Cursor, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_active_tab_change_restores_focus_and_acknowledges_landing_unseen_attention() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = state.events_from_active_tab(
            10,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
        );
        assert_eq!(
            events,
            vec![
                StateEvent::ActiveTabChanged { active_tab_id: 10 },
                StateEvent::BecameActive,
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                    }),
                    acknowledge_existing_attention: true,
                },
            ]
        );

        let _ = state.apply_all(&events);
        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(current_tab.active_focus_pane_id, Some(42));
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
    }

    #[test]
    fn test_active_tab_change_keeps_red_when_landing_on_other_pane() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                }),
                pane_state_by_pane: HashMap::from([
                    (
                        42,
                        pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                    ),
                    (
                        43,
                        pane_state(Agent::Codex, AgentPanePhase::AttentionSeen, PaneFocus::Unfocused, 2),
                    ),
                ]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = state.events_from_active_tab(
            10,
            Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
            }),
        );
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);
        assert_eq!(current_tab.active_focus_pane_id, Some(43));
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionUnseen)
        );
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&43)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
        assert_eq!(
            current_tab.current_row_display(true),
            (Cmd::agent(Agent::Claude, AgentState::NeedsAttention), TabIndicator::Red,)
        );
    }

    #[test]
    fn test_active_tab_change_without_host_focus_acknowledges_matching_pane_on_first_pane_update() {
        let mut state = State {
            plugin_id: 7,
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let activation_events = state.events_from_active_tab(10, None);
        assert_eq!(
            activation_events,
            vec![
                StateEvent::ActiveTabChanged { active_tab_id: 10 },
                StateEvent::BecameActive,
            ]
        );
        let _ = state.apply_all(&activation_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(current_tab.pending_activation_focus_ack);
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);

        let pane_update_events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![
                    plugin_pane(7),
                    terminal_pane_with_command(42, true, "claude"),
                    terminal_pane_with_command(43, false, "/bin/zsh"),
                ],
            )]),
        );
        assert_eq!(
            pane_update_events,
            vec![
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                    }),
                    acknowledge_existing_attention: true,
                },
                StateEvent::SyncRequested,
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(!current_tab.pending_activation_focus_ack);
        assert_eq!(current_tab.active_focus_pane_id, Some(42));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
    }

    #[test]
    fn test_active_tab_pipe_acknowledges_landing_focus_after_tab_update_pending_ack() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let mut tabs = vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..tab_with_name(10, 0, "a")
        }];
        let tab_update_events = state.events_from_tab_update(&mut tabs, None);
        assert_eq!(
            tab_update_events,
            vec![
                StateEvent::AllTabsReplaced { new_tabs: tabs.clone() },
                StateEvent::TopologyChanged,
                StateEvent::BecameActive,
                StateEvent::SyncRequested,
            ]
        );
        let _ = state.apply_all(&tab_update_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(current_tab.pending_activation_focus_ack);
        assert_eq!(state.known_active_tab_id, Some(10));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);

        let pipe_events = state.events_from_active_tab(
            10,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
        );
        assert_eq!(
            pipe_events,
            vec![StateEvent::FocusChanged {
                new_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                acknowledge_existing_attention: true,
            }]
        );
        let _ = state.apply_all(&pipe_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(!current_tab.pending_activation_focus_ack);
        assert_eq!(current_tab.active_focus_pane_id, Some(42));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
    }

    #[test]
    fn test_active_tab_pipe_reconciles_real_landing_focus_after_stale_tab_update_focus() {
        let mut state = State {
            known_active_tab_id: Some(20),
            all_tabs: vec![TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                }),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let mut tabs = vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..tab_with_name(10, 0, "a")
        }];
        let tab_update_events = state.events_from_tab_update(
            &mut tabs,
            Some(FocusedPane {
                id: 43,
                label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
            }),
        );
        let _ = state.apply_all(&tab_update_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert!(!current_tab.pending_activation_focus_ack);
        assert_eq!(current_tab.active_focus_pane_id, Some(43));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Red);

        let pipe_events = state.events_from_active_tab(
            10,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
        );
        assert_eq!(
            pipe_events,
            vec![StateEvent::FocusChanged {
                new_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                acknowledge_existing_attention: true,
            }]
        );
        let _ = state.apply_all(&pipe_events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.active_focus_pane_id, Some(42));
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab
                .pane_state_by_pane
                .get(&42)
                .map(|pane_state| pane_state.phase),
            Some(AgentPanePhase::AttentionSeen)
        );
    }

    #[test]
    fn test_active_tab_pipe_matching_landing_focus_with_no_unseen_attention_is_noop() {
        let state = State {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionSeen, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = state.events_from_active_tab(
            10,
            Some(FocusedPane {
                id: 42,
                label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
            }),
        );

        assert_eq!(events, vec![]);
    }

    #[test]
    fn test_attention_after_focus_restore_is_seen_immediately() {
        let mut state = State {
            known_active_tab_id: Some(10),
            current_tab: Some(CurrentTab {
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("claude".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_ids: std::iter::once(42).collect(),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = state.events_from_agent_event(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Claude,
            kind: AgentEventKind::Idle,
        });
        let _ = state.apply_all(&events);

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Claude, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_events_from_pane_update_ignores_stale_title_when_command_is_shell() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::Title("Cursor …".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Cursor, AgentPanePhase::AttentionSeen, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let _ = state.apply_all(&[StateEvent::AgentLost { pane_id: 42 }]);
        let manifest = manifest(vec![(
            0,
            vec![PaneInfo {
                id: 42,
                is_focused: true,
                terminal_command: Some("/bin/zsh".to_string()),
                title: "Cursor Agent".to_string(),
                ..Default::default()
            }],
        )]);

        let events = state.events_from_pane_update(&manifest, noop_pane_cwd);

        assert_eq!(events, vec![]);
    }

    #[test]
    fn test_events_from_pane_closed_removes_tracked_agent_immediately() {
        let state = State {
            current_tab: Some(CurrentTab {
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Cursor, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        assert_eq!(
            state.events_from_pane_closed(42),
            vec![StateEvent::AgentLost { pane_id: 42 }]
        );
    }

    #[test]
    fn test_events_from_pane_update_clears_tracked_agent_when_process_changes() {
        let state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let manifest = manifest(vec![(
            0,
            vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
        )]);
        let events = state.events_from_pane_update(&manifest, noop_pane_cwd);
        assert_eq!(
            events,
            vec![
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane { id: 42, label: None }),
                    acknowledge_existing_attention: false,
                },
                StateEvent::AgentLost { pane_id: 42 },
                StateEvent::SyncRequested,
            ]
        );
    }

    #[test]
    fn test_events_from_pane_update_clears_unfocused_tracked_agent_when_process_changes() {
        let state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                }),
                active_focus_pane_id: Some(43),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let manifest = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, false, "/bin/zsh"),
                terminal_pane_with_command(43, true, "cargo"),
            ],
        )]);
        let events = state.events_from_pane_update(&manifest, noop_pane_cwd);
        assert_eq!(
            events,
            vec![StateEvent::AgentLost { pane_id: 42 }, StateEvent::SyncRequested,]
        );
    }

    #[test]
    fn test_apply_pane_update_keeps_idle_agent_when_title_becomes_path() {
        let mut state = State {
            plugin_id: 7,
            all_tabs: vec![tab_with_name(10, 0, "a")],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                }),
                active_focus_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionSeen, PaneFocus::Focused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            ..Default::default()
        };

        let events = apply_pane_update(
            &mut state,
            &manifest(vec![(
                0,
                vec![plugin_pane(7), terminal_pane_with_title(42, true, "/tmp/project")],
            )]),
        );
        assert_eq!(
            events,
            vec![
                StateEvent::FocusChanged {
                    new_pane: Some(FocusedPane { id: 42, label: None }),
                    acknowledge_existing_attention: false,
                },
                StateEvent::SyncRequested,
            ]
        );

        assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert_eq!(current_tab.tab_indicator(), TabIndicator::Empty);
        assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }
}
