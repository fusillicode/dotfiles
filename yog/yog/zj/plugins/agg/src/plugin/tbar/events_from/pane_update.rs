use std::collections::HashSet;
use std::path::PathBuf;

use agg::AgentState;
use agg::Cmd;
use agg::GitStat;
use agg::TabIndicator;
use zellij_tile::prelude::PaneInfo;
use zellij_tile::prelude::PaneManifest;
use zellij_tile::prelude::TabInfo;

use crate::plugin::pane::FocusedPane;
use crate::plugin::pane::FocusedPaneLabel;
use crate::plugin::tbar::Event;
use crate::plugin::tbar::StateSnapshotPayload;
use crate::plugin::tbar::TbarState;
use crate::plugin::tbar::current_tab::CurrentTab;

pub fn derive(
    state: &TbarState,
    manifest: &PaneManifest,
    mut resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
) -> Vec<Event> {
    let Some(tab_pos) = current_tab_position_in_manifest(state.plugin_id, manifest) else {
        return vec![];
    };
    let Some(panes) = manifest.panes.get(&tab_pos) else {
        return vec![];
    };

    let mut events = vec![];
    let current_tab_id = state.current_tab.as_ref().map(|current_tab| current_tab.tab_id);
    let discovered_tab_id =
        stable_tab_for_manifest_position(&state.all_tabs, tab_pos, panes, state.current_tab.is_some())
            .map(|tab| tab.tab_id);

    let bootstrapped_current_tab = bootstrap_current_tab_for_pane_update(state.current_tab.as_ref(), discovered_tab_id);
    if let Some(current_tab) = bootstrapped_current_tab.as_ref() {
        events.push(Event::TabCreated {
            tab_id: current_tab.tab_id,
        });
    }

    if let (Some(current_id), Some(discovered_id)) = (current_tab_id, discovered_tab_id)
        && !state.all_tabs.iter().any(|tab| tab.tab_id == current_id)
    {
        events.push(Event::TabRemapped {
            new_tab_id: discovered_id,
        });
    }

    let Some(current_tab) = state.current_tab.as_ref().or(bootstrapped_current_tab.as_ref()) else {
        return events;
    };

    let display_tab_id = current_tab_id
        .filter(|id| state.all_tabs.iter().any(|tab| tab.tab_id == *id))
        .or(discovered_tab_id);
    let display_tab_is_active = state.known_active_tab_id.map_or_else(
        || crate::plugin::tbar::queries::current_tab_is_active_in(&state.all_tabs, display_tab_id),
        |active_tab_id| display_tab_id == Some(active_tab_id),
    );

    let new_pane_ids: HashSet<u32> = displayable_terminal_panes(panes).map(|pane| pane.id).collect();
    let new_focused_pane = displayable_terminal_panes(panes)
        .find(|pane| pane.is_focused)
        .and_then(crate::plugin::pane::focused_pane_from_pane_info);
    let new_display_pane = display_pane_for_manifest_tab(panes, display_tab_is_active, None);

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
                events.push(Event::AgentLost {
                    pane_id: *removed_pane_id,
                });
            }
        }
        events.push(Event::PanesChanged {
            observed_pane_ids,
            retained_pane_ids,
        });
    }

    let new_focus_pane_id = new_focused_pane.as_ref().map(|pane| pane.id);
    let focused_pane_id_changed = new_focus_pane_id != current_tab.focused_pane.as_ref().map(|pane| pane.id);
    let display_metadata_changed = new_display_pane != current_tab.focused_pane;
    let focus_tracking_changed = state.current_tab_is_active() && current_tab.active_focus_pane_id != new_focus_pane_id;
    let pending_activation_focus_ack =
        state.current_tab_is_active() && current_tab.pending_activation_focus_ack && new_focus_pane_id.is_some();
    if display_metadata_changed || focus_tracking_changed || pending_activation_focus_ack {
        events.push(Event::FocusChanged {
            new_pane: new_display_pane.clone(),
            acknowledge_existing_attention: pending_activation_focus_ack
                || state.current_tab_is_active() && focused_pane_id_changed && new_focus_pane_id.is_some(),
        });
    }

    events.extend(agent_changes_from_manifest(
        current_tab,
        new_display_pane.as_ref(),
        panes,
        &new_pane_ids,
    ));

    if let Some(event) = display_cwd_change(
        state,
        current_tab,
        display_tab_is_active,
        display_metadata_changed,
        new_display_pane.as_ref(),
        &mut resolve_pane_cwd,
    ) {
        events.push(event);
    }

    let remote_events = remote_tab_changes(state, manifest, tab_pos, &mut resolve_pane_cwd);
    events.extend(remote_events);

    push_sync_request(state.current_tab.is_some(), state.sync_requested, &mut events);

    events
}

fn display_cwd_change(
    state: &TbarState,
    current_tab: &CurrentTab,
    display_tab_is_active: bool,
    display_metadata_changed: bool,
    display_pane: Option<&FocusedPane>,
    resolve_pane_cwd: &mut impl FnMut(u32) -> Option<PathBuf>,
) -> Option<Event> {
    let display_pane = display_pane?;
    let pane_id = current_tab.display_cwd_pane_id(display_tab_is_active, Some(display_pane.id))?;
    // Display metadata can move to an agent before its cwd is cached; retry until that pane has a cwd.
    let cached_display_cwd = state.cwds_by_pane.get(&pane_id);
    if !display_metadata_changed && cached_display_cwd.is_some() {
        return None;
    }
    let new_cwd = resolve_pane_cwd(pane_id).or_else(|| cached_display_cwd.cloned())?;
    (cached_display_cwd != Some(&new_cwd) || current_tab.cwd.as_ref() != Some(&new_cwd))
        .then_some(Event::CwdChanged { pane_id, new_cwd })
}

fn displayable_terminal_panes(panes: &[PaneInfo]) -> impl Iterator<Item = &PaneInfo> {
    panes
        .iter()
        .filter(|pane| crate::plugin::pane::is_displayable_terminal_pane(pane))
}

fn display_pane_for_manifest_tab(
    panes: &[PaneInfo],
    tab_is_active: bool,
    existing_remote: Option<&StateSnapshotPayload>,
) -> Option<FocusedPane> {
    let mut focused_pane = None;
    let mut first_displayable_terminal_pane = None;
    for pane in displayable_terminal_panes(panes) {
        if first_displayable_terminal_pane.is_none() {
            first_displayable_terminal_pane = crate::plugin::pane::focused_pane_from_pane_info(pane);
        }
        if pane.is_focused {
            focused_pane = crate::plugin::pane::focused_pane_from_pane_info(pane);
        }
    }
    if tab_is_active {
        focused_pane
    } else {
        if let Some(snapshot_pane) = existing_remote
            .filter(|remote| remote.seq > 0)
            .and_then(|remote| manifest_agent_pane_for_snapshot(panes, remote))
        {
            return Some(snapshot_pane);
        }
        let focused_pane_is_agent = focused_pane.as_ref().is_some_and(|focused_pane| {
            panes
                .iter()
                .find(|pane| pane.id == focused_pane.id && !pane.is_plugin)
                .and_then(|pane| crate::plugin::pane::detected_agent_from_pane_info(pane, focused_pane))
                .is_some()
        });
        if focused_pane_is_agent {
            focused_pane
        } else {
            manifest_agent_pane(panes)
                .or(focused_pane)
                .or(first_displayable_terminal_pane)
        }
    }
}

/// Issue: a remote snapshot can report several panes for the same agent.
/// Prefer its focused pane when it still detects as that agent, then fall back to pane metadata.
fn manifest_agent_pane_for_snapshot(panes: &[PaneInfo], snapshot: &StateSnapshotPayload) -> Option<FocusedPane> {
    let agent = snapshot.cmd.tracked_agent()?;
    let pane_for_id = |pane_id| -> Option<FocusedPane> {
        let pane = displayable_terminal_panes(panes).find(|pane| pane.id == pane_id)?;
        let focused_pane = crate::plugin::pane::focused_pane_from_pane_info(pane)?;
        (crate::plugin::pane::detected_agent_from_pane_info(pane, &focused_pane) == Some(agent)).then_some(focused_pane)
    };
    if let Some(pane_id) = snapshot.focused_pane_id
        && let Some(focused_pane) = pane_for_id(pane_id)
    {
        return Some(focused_pane);
    }
    let pane_id = snapshot
        .pane_agents
        .iter()
        .find(|pane| pane.agent == agent && pane.indicator == snapshot.indicator)
        .or_else(|| snapshot.pane_agents.iter().find(|pane| pane.agent == agent))
        .map(|pane| pane.pane_id)
        .or(snapshot.focused_pane_id)?;
    pane_for_id(pane_id)
}

/// Issue: inactive manifest fallback used the last focused pane even when an idle agent existed.
/// Prefer any agent pane so no-dot agent tabs still show the agent path.
fn manifest_agent_pane(panes: &[PaneInfo]) -> Option<FocusedPane> {
    displayable_terminal_panes(panes).find_map(|pane| {
        let focused_pane = crate::plugin::pane::focused_pane_from_pane_info(pane)?;
        crate::plugin::pane::detected_agent_from_pane_info(pane, &focused_pane).map(|_| focused_pane)
    })
}

fn remote_tab_changes(
    state: &TbarState,
    manifest: &PaneManifest,
    current_tab_pos: usize,
    resolve_pane_cwd: &mut impl FnMut(u32) -> Option<PathBuf>,
) -> Vec<Event> {
    let mut events = vec![];
    for (&tab_pos, panes) in &manifest.panes {
        if tab_pos == current_tab_pos {
            continue;
        }
        let Some(source_plugin_id) = panes.iter().find(|pane| pane.is_plugin).map(|pane| pane.id) else {
            continue;
        };
        let existing_remote = state.other_tabs.get(&source_plugin_id);
        let Some(tab) = stable_tab_for_manifest_position(&state.all_tabs, tab_pos, panes, existing_remote.is_some())
        else {
            continue;
        };
        if state.current_tab_id() == Some(tab.tab_id) {
            continue;
        }
        let Some(display_pane) = display_pane_for_manifest_tab(panes, tab.active, existing_remote) else {
            continue;
        };
        let Some(pane) = panes.iter().find(|pane| pane.id == display_pane.id && !pane.is_plugin) else {
            continue;
        };

        let cwd = resolve_pane_cwd(display_pane.id).or_else(|| state.cwds_by_pane.get(&display_pane.id).cloned());
        let manifest_snapshot = snapshot_from_manifest_tab(tab.tab_id, &display_pane, pane, cwd);
        let Some(snapshot) = remote_manifest_update(existing_remote, manifest_snapshot) else {
            continue;
        };
        let evict_ids = state
            .other_tabs
            .iter()
            .filter(|&(plugin_id, remote)| *plugin_id != source_plugin_id && remote.tab_id == snapshot.tab_id)
            .map(|(&plugin_id, _)| plugin_id)
            .collect();
        events.push(Event::RemoteTabUpdated {
            source_plugin_id,
            snapshot,
            evict_ids,
        });
    }
    events
}

fn snapshot_from_manifest_tab(
    tab_id: usize,
    display_pane: &FocusedPane,
    pane: &PaneInfo,
    cwd: Option<PathBuf>,
) -> StateSnapshotPayload {
    let cmd = crate::plugin::pane::detected_agent_from_pane_info(pane, display_pane).map_or_else(
        || {
            display_pane.label.as_ref().map_or(Cmd::None, |label| match label {
                FocusedPaneLabel::TerminalCommand(command) | FocusedPaneLabel::Title(command) => {
                    Cmd::Running(command.clone())
                }
            })
        },
        |agent| Cmd::agent(agent, AgentState::Acknowledged),
    );
    StateSnapshotPayload {
        tab_id,
        seq: 0,
        focused_pane_id: Some(display_pane.id),
        cwd,
        indicator: TabIndicator::from_cmd(&cmd),
        cmd,
        git_stat: GitStat::default(),
        pane_agents: vec![],
    }
}

fn push_sync_request(current_tab_exists: bool, sync_requested: bool, events: &mut Vec<Event>) {
    let has_resetter = events
        .iter()
        .any(|event| matches!(event, Event::TabCreated { .. } | Event::TabRemapped { .. }));
    if has_resetter || current_tab_exists && !sync_requested {
        events.push(Event::SyncRequested);
    }
}

/// Issue: `TabUpdate` and `PaneManifest` can describe different moments while `znt`
/// moves a focused new tab. Solution: ignore unstable position joins until plugin id anchors the tab.
fn stable_tab_for_manifest_position<'a>(
    tabs: &'a [TabInfo],
    tab_pos: usize,
    panes: &[PaneInfo],
    allow_focused_inactive: bool,
) -> Option<&'a TabInfo> {
    let tab = tabs.iter().find(|tab| tab.position == tab_pos)?;
    if !allow_focused_inactive && panes.iter().any(|pane| pane.is_focused) && !tab.active {
        return None;
    }
    Some(tab)
}

/// Issue: real pipe snapshots own agent state but can arrive before cwd. Solution:
/// hydrate only same-tab manifest metadata and keep snapshot seq/status/git state.
fn remote_manifest_update(
    existing: Option<&StateSnapshotPayload>,
    manifest: StateSnapshotPayload,
) -> Option<StateSnapshotPayload> {
    let Some(existing) = existing else {
        return Some(manifest);
    };

    if existing.seq == 0 {
        let unchanged = existing.tab_id == manifest.tab_id
            && existing.cwd == manifest.cwd
            && existing.cmd == manifest.cmd
            && existing.indicator == manifest.indicator
            && existing.focused_pane_id == manifest.focused_pane_id;
        return (!unchanged).then_some(manifest);
    }

    if existing.tab_id != manifest.tab_id {
        return None;
    }

    let StateSnapshotPayload {
        focused_pane_id,
        cwd,
        cmd,
        indicator,
        ..
    } = manifest;
    let seed_command = matches!(&existing.cmd, Cmd::None) && !matches!(&cmd, Cmd::None);
    let hydrate_matches_command = existing.cmd.matches_manifest_hydration(&cmd);
    let hydrate_path = hydrate_matches_command && cwd.is_some() && existing.cwd != cwd;
    let hydrate_focus =
        hydrate_matches_command && focused_pane_id.is_some() && existing.focused_pane_id != focused_pane_id;
    if !(hydrate_path || hydrate_focus || seed_command) {
        return None;
    }

    let mut merged = existing.clone();
    if hydrate_path {
        merged.cwd = cwd;
    }
    if hydrate_focus {
        merged.focused_pane_id = focused_pane_id;
    }
    if seed_command {
        merged.cmd = cmd;
        merged.indicator = indicator;
    }

    Some(merged)
}

fn current_tab_position_in_manifest(plugin_id: u32, manifest: &PaneManifest) -> Option<usize> {
    manifest.panes.iter().find_map(|(tab_pos, panes)| {
        panes
            .iter()
            .any(|pane| pane.is_plugin && pane.id == plugin_id)
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

fn agent_changes_from_manifest(
    current_tab: &CurrentTab,
    display_pane: Option<&FocusedPane>,
    panes: &[PaneInfo],
    surviving_pane_ids: &HashSet<u32>,
) -> Vec<Event> {
    let mut events = vec![];
    let Some(display_pane) = display_pane else {
        return events;
    };
    let Some(pane) = panes.iter().find(|pane| pane.id == display_pane.id && !pane.is_plugin) else {
        return events;
    };
    if pane.exited || pane.is_held {
        return events;
    }

    let stored_agent = current_tab
        .pane_state_by_pane
        .get(&display_pane.id)
        .map(|pane_state| pane_state.agent);
    let detected_agent = crate::plugin::pane::detected_agent_from_pane_info(pane, display_pane);
    let has_terminal_command = pane
        .terminal_command
        .as_ref()
        .is_some_and(|command| !command.trim().is_empty());

    match (stored_agent, detected_agent) {
        (Some(stored_agent), Some(detected_agent)) if stored_agent != detected_agent => {
            events.push(Event::AgentLost {
                pane_id: display_pane.id,
            });
            events.push(Event::AgentDetected {
                pane_id: display_pane.id,
                agent: detected_agent,
            });
        }
        (None, Some(detected_agent)) => {
            events.push(Event::AgentDetected {
                pane_id: display_pane.id,
                agent: detected_agent,
            });
        }
        (Some(_), None) if has_terminal_command => {
            events.push(Event::AgentLost {
                pane_id: display_pane.id,
            });
        }
        _ => {}
    }

    for (&pane_id, pane_state) in &current_tab.pane_state_by_pane {
        if pane_id == display_pane.id || !surviving_pane_ids.contains(&pane_id) {
            continue;
        }
        let Some(other_pane) = panes.iter().find(|pane| pane.id == pane_id && !pane.is_plugin) else {
            continue;
        };
        if other_pane.exited || other_pane.is_held {
            continue;
        }
        let detected_agent = crate::plugin::pane::focused_pane_from_pane_info(other_pane)
            .as_ref()
            .and_then(|focused_pane| crate::plugin::pane::detected_agent_from_pane_info(other_pane, focused_pane));
        let has_terminal_command = other_pane
            .terminal_command
            .as_ref()
            .is_some_and(|command| !command.trim().is_empty());
        if has_terminal_command && detected_agent != Some(pane_state.agent) {
            events.push(Event::AgentLost { pane_id });
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use agg::AgentState;
    use agg::Cmd;
    use agg::GitStat;
    use agg::TabIndicator;
    use ytil_agents::agent::Agent;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::TabInfo;

    use crate::plugin::pane::FocusedPane;
    use crate::plugin::pane::FocusedPaneLabel;
    use crate::plugin::tbar::Event;
    use crate::plugin::tbar::PaneAgentSnapshot;
    use crate::plugin::tbar::StateSnapshotPayload;
    use crate::plugin::tbar::TbarState;
    use crate::plugin::tbar::current_tab::AgentPanePhase;
    use crate::plugin::tbar::current_tab::CurrentTab;
    use crate::plugin::tbar::current_tab::PaneFocus;
    use crate::plugin::tbar::events_from::pane_update::*;
    use crate::plugin::tbar::test_support::*;

    #[test]
    fn test_apply_pane_update_first_detected_agent_starts_seen_until_busy() {
        let mut state = TbarState {
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
        pretty_assertions::assert_eq!(
            events,
            vec![
                Event::AgentDetected {
                    pane_id: 42,
                    agent: Agent::Claude,
                },
                Event::SyncRequested,
            ]
        );

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Claude, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_apply_pane_update_bootstraps_current_tab_and_detects_codex_on_first_update() {
        let mut state = TbarState {
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
        pretty_assertions::assert_eq!(
            events,
            vec![
                Event::TabCreated { tab_id: 10 },
                Event::PanesChanged {
                    observed_pane_ids: std::iter::once(42).collect(),
                    retained_pane_ids: std::iter::once(42).collect(),
                },
                Event::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                Event::AgentDetected {
                    pane_id: 42,
                    agent: Agent::Codex,
                },
                Event::SyncRequested,
            ]
        );

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_apply_pane_update_bootstraps_current_tab_without_detecting_non_agent_command() {
        let mut state = TbarState {
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
        pretty_assertions::assert_eq!(
            events,
            vec![
                Event::TabCreated { tab_id: 10 },
                Event::PanesChanged {
                    observed_pane_ids: std::iter::once(42).collect(),
                    retained_pane_ids: std::iter::once(42).collect(),
                },
                Event::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("cargo".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                Event::SyncRequested,
            ]
        );

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        assert2::assert!(current_tab.pane_state_by_pane.is_empty());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::NoAgent);
        pretty_assertions::assert_eq!(current_tab.display_cmd(), Cmd::Running("cargo".to_string()));
    }

    #[test]
    fn test_apply_pane_update_bootstraps_current_tab_and_detects_codex_from_title_on_first_update() {
        let mut state = TbarState {
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
        pretty_assertions::assert_eq!(
            events,
            vec![
                Event::TabCreated { tab_id: 10 },
                Event::PanesChanged {
                    observed_pane_ids: std::iter::once(42).collect(),
                    retained_pane_ids: std::iter::once(42).collect(),
                },
                Event::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::Title("codex".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                Event::AgentDetected {
                    pane_id: 42,
                    agent: Agent::Codex,
                },
                Event::SyncRequested,
            ]
        );

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }

    #[test]
    fn test_apply_pane_update_waits_when_focused_manifest_position_maps_to_inactive_tab() {
        let state = TbarState {
            plugin_id: 7,
            all_tabs: vec![
                tab_with_name(10, 0, "old-left"),
                tab_with_name(20, 1, "stale-position"),
                TabInfo {
                    active: true,
                    ..tab_with_name(30, 2, "new-tab")
                },
            ],
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![(
                1,
                vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
            )]),
            noop_pane_cwd,
        );

        pretty_assertions::assert_eq!(events, vec![]);
    }

    #[test]
    fn test_apply_pane_update_does_not_hydrate_remote_tab_from_mismatched_active_position() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(30),
            all_tabs: vec![
                tab_with_name(10, 0, "current"),
                tab_with_name(20, 1, "stale-position"),
                TabInfo {
                    active: true,
                    ..tab_with_name(30, 2, "new-tab")
                },
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            sync_requested: true,
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, false, "/bin/zsh")],
                ),
                (1, vec![plugin_pane(8), terminal_pane_with_command(43, true, "codex")]),
            ]),
            |pane_id| (pane_id == 43).then(|| PathBuf::from("/Users/me/new-tab")),
        );

        pretty_assertions::assert_eq!(events, vec![]);
    }

    #[test]
    fn test_apply_pane_update_bootstraps_inactive_tab_from_first_displayable_terminal() {
        let mut state = TbarState {
            plugin_id: 7,
            all_tabs: vec![
                tab_with_name(10, 0, "Tab #1"),
                TabInfo {
                    active: true,
                    ..tab_with_name(20, 1, "Tab #2")
                },
            ],
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };
        let cwd = PathBuf::from("/Users/me/project");
        let events = derive(
            &state,
            &manifest(vec![(
                0,
                vec![plugin_pane(7), terminal_pane_with_command(42, false, "codex")],
            )]),
            |pane_id| (pane_id == 42).then(|| cwd.clone()),
        );
        pretty_assertions::assert_eq!(
            events,
            vec![
                Event::TabCreated { tab_id: 10 },
                Event::PanesChanged {
                    observed_pane_ids: std::iter::once(42).collect(),
                    retained_pane_ids: std::iter::once(42).collect(),
                },
                Event::FocusChanged {
                    new_pane: Some(FocusedPane {
                        id: 42,
                        label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                    }),
                    acknowledge_existing_attention: false,
                },
                Event::AgentDetected {
                    pane_id: 42,
                    agent: Agent::Codex,
                },
                Event::CwdChanged {
                    pane_id: 42,
                    new_cwd: PathBuf::from("/Users/me/project"),
                },
                Event::SyncRequested,
            ]
        );

        let _ = state.apply_all(&events);
        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.cwd, Some(PathBuf::from("/Users/me/project")));
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        assert2::assert!(let Some(row) = state.frame.first());
        pretty_assertions::assert_eq!(row.path_label, "~/project");
        pretty_assertions::assert_eq!(row.cmd, Cmd::agent(Agent::Codex, AgentState::Acknowledged));
    }

    #[test]
    fn test_apply_pane_update_retries_missing_inactive_agent_cwd() {
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "Tab #1")],
            current_tab: Some(CurrentTab {
                pane_ids: [42, 43].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 43,
                    label: Some(FocusedPaneLabel::TerminalCommand("zsh".to_string())),
                }),
                cwd: Some(PathBuf::from("/Users/me/focused")),
                last_focused_agent_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([(
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::AttentionSeen, PaneFocus::Unfocused, 1),
                )]),
                ..CurrentTab::new(10)
            }),
            sync_requested: true,
            cwds_by_pane: HashMap::from([(43, PathBuf::from("/Users/me/focused"))]),
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };
        let manifest = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(42, false, "codex"),
                terminal_pane_with_command(43, true, "/bin/zsh"),
            ],
        )]);

        let first_events = derive(&state, &manifest, |_| None);
        pretty_assertions::assert_eq!(
            first_events,
            vec![Event::FocusChanged {
                new_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                }),
                acknowledge_existing_attention: false,
            }]
        );

        let _ = state.apply_all(&first_events);
        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.display_cwd(false, &state.cwds_by_pane), None);

        let second_events = derive(&state, &manifest, |pane_id| {
            (pane_id == 42).then(|| PathBuf::from("/Users/me/agent"))
        });
        pretty_assertions::assert_eq!(
            second_events,
            vec![Event::CwdChanged {
                pane_id: 42,
                new_cwd: PathBuf::from("/Users/me/agent"),
            }]
        );
    }

    #[test]
    fn test_apply_pane_update_inactive_cwd_uses_busy_agent_winner() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(20),
            all_tabs: vec![tab_with_name(10, 0, "Tab #1")],
            current_tab: Some(CurrentTab {
                pane_ids: [41, 42].into_iter().collect(),
                focused_pane: Some(FocusedPane {
                    id: 42,
                    label: Some(FocusedPaneLabel::TerminalCommand("codex".to_string())),
                }),
                cwd: Some(PathBuf::from("/Users/me/codex")),
                last_focused_agent_pane_id: Some(42),
                pane_state_by_pane: HashMap::from([
                    (
                        41,
                        pane_state(Agent::Claude, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
                    ),
                    (
                        42,
                        pane_state(Agent::Codex, AgentPanePhase::AttentionSeen, PaneFocus::Focused, 2),
                    ),
                ]),
                ..CurrentTab::new(10)
            }),
            sync_requested: true,
            cwds_by_pane: HashMap::from([(42, PathBuf::from("/Users/me/codex"))]),
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };
        let manifest = manifest(vec![(
            0,
            vec![
                plugin_pane(7),
                terminal_pane_with_command(41, false, "claude"),
                terminal_pane_with_command(42, true, "codex"),
            ],
        )]);

        let events = derive(&state, &manifest, |pane_id| {
            (pane_id == 41).then(|| PathBuf::from("/Users/me/claude"))
        });

        pretty_assertions::assert_eq!(
            events,
            vec![Event::CwdChanged {
                pane_id: 41,
                new_cwd: PathBuf::from("/Users/me/claude"),
            }]
        );
    }

    #[test]
    fn test_apply_pane_update_hydrates_remote_tabs_from_manifest() {
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };
        let remote_cwd = PathBuf::from("/Users/me/project");

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (1, vec![plugin_pane(8), terminal_pane_with_command(43, false, "codex")]),
            ]),
            |pane_id| (pane_id == 43).then(|| remote_cwd.clone()),
        );
        pretty_assertions::assert_eq!(
            events,
            vec![Event::RemoteTabUpdated {
                source_plugin_id: 8,
                snapshot: StateSnapshotPayload {
                    tab_id: 20,
                    seq: 0,
                    focused_pane_id: Some(43),
                    cwd: Some(PathBuf::from("/Users/me/project")),
                    cmd: Cmd::agent(Agent::Codex, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
                evict_ids: vec![],
            }]
        );

        let _ = state.apply_all(&events);
        assert2::assert!(let Some(row) = state.frame.get(1));
        pretty_assertions::assert_eq!(row.path_label, "~/project");
        pretty_assertions::assert_eq!(row.cmd, Cmd::agent(Agent::Codex, AgentState::Acknowledged));
    }

    #[test]
    fn test_apply_pane_update_remote_inactive_tab_prefers_idle_agent_pane_cwd() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            other_tabs: HashMap::from([(8, snapshot(20, 0, Cmd::None, TabIndicator::NoAgent))]),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (
                    1,
                    vec![
                        plugin_pane(8),
                        terminal_pane_with_command(43, false, "codex"),
                        terminal_pane_with_command(44, true, "/bin/zsh"),
                    ],
                ),
            ]),
            |pane_id| match pane_id {
                43 => Some(PathBuf::from("/Users/me/agent")),
                44 => Some(PathBuf::from("/Users/me/focused")),
                _ => None,
            },
        );

        pretty_assertions::assert_eq!(
            events,
            vec![Event::RemoteTabUpdated {
                source_plugin_id: 8,
                snapshot: StateSnapshotPayload {
                    tab_id: 20,
                    seq: 0,
                    focused_pane_id: Some(43),
                    cwd: Some(PathBuf::from("/Users/me/agent")),
                    cmd: Cmd::agent(Agent::Codex, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
                evict_ids: vec![],
            }]
        );
    }

    #[test]
    fn test_apply_pane_update_remote_inactive_tab_keeps_existing_agent_cwd_aligned() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            other_tabs: HashMap::from([(
                8,
                StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(44),
                    cwd: Some(PathBuf::from("/Users/me/old")),
                    cmd: Cmd::agent(Agent::Claude, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![
                        PaneAgentSnapshot {
                            pane_id: 43,
                            agent: Agent::Codex,
                            indicator: TabIndicator::Seen,
                        },
                        PaneAgentSnapshot {
                            pane_id: 45,
                            agent: Agent::Claude,
                            indicator: TabIndicator::Seen,
                        },
                    ],
                },
            )]),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (
                    1,
                    vec![
                        plugin_pane(8),
                        terminal_pane_with_command(43, false, "codex"),
                        terminal_pane_with_command(44, true, "/bin/zsh"),
                        terminal_pane_with_command(45, false, "claude"),
                    ],
                ),
            ]),
            |pane_id| match pane_id {
                43 => Some(PathBuf::from("/Users/me/codex")),
                44 => Some(PathBuf::from("/Users/me/focused")),
                45 => Some(PathBuf::from("/Users/me/claude")),
                _ => None,
            },
        );

        pretty_assertions::assert_eq!(
            events,
            vec![Event::RemoteTabUpdated {
                source_plugin_id: 8,
                snapshot: StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(45),
                    cwd: Some(PathBuf::from("/Users/me/claude")),
                    cmd: Cmd::agent(Agent::Claude, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![
                        PaneAgentSnapshot {
                            pane_id: 43,
                            agent: Agent::Codex,
                            indicator: TabIndicator::Seen,
                        },
                        PaneAgentSnapshot {
                            pane_id: 45,
                            agent: Agent::Claude,
                            indicator: TabIndicator::Seen,
                        },
                    ],
                },
                evict_ids: vec![],
            }]
        );
    }

    #[test]
    fn test_apply_pane_update_remote_inactive_tab_prefers_snapshot_focus_for_same_agent() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            other_tabs: HashMap::from([(
                8,
                StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(45),
                    cwd: Some(PathBuf::from("/Users/me/old")),
                    cmd: Cmd::agent(Agent::Codex, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![
                        PaneAgentSnapshot {
                            pane_id: 43,
                            agent: Agent::Codex,
                            indicator: TabIndicator::Seen,
                        },
                        PaneAgentSnapshot {
                            pane_id: 45,
                            agent: Agent::Codex,
                            indicator: TabIndicator::Seen,
                        },
                    ],
                },
            )]),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (
                    1,
                    vec![
                        plugin_pane(8),
                        terminal_pane_with_command(43, false, "codex"),
                        terminal_pane_with_command(44, true, "/bin/zsh"),
                        terminal_pane_with_command(45, false, "codex"),
                    ],
                ),
            ]),
            |pane_id| match pane_id {
                43 => Some(PathBuf::from("/Users/me/other-codex")),
                44 => Some(PathBuf::from("/Users/me/focused")),
                45 => Some(PathBuf::from("/Users/me/codex-winner")),
                _ => None,
            },
        );

        pretty_assertions::assert_eq!(
            events,
            vec![Event::RemoteTabUpdated {
                source_plugin_id: 8,
                snapshot: StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(45),
                    cwd: Some(PathBuf::from("/Users/me/codex-winner")),
                    cmd: Cmd::agent(Agent::Codex, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![
                        PaneAgentSnapshot {
                            pane_id: 43,
                            agent: Agent::Codex,
                            indicator: TabIndicator::Seen,
                        },
                        PaneAgentSnapshot {
                            pane_id: 45,
                            agent: Agent::Codex,
                            indicator: TabIndicator::Seen,
                        },
                    ],
                },
                evict_ids: vec![],
            }]
        );
    }

    #[test]
    fn test_apply_pane_update_remote_inactive_tab_skips_cwd_from_different_agent() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            other_tabs: HashMap::from([(
                8,
                StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(45),
                    cwd: Some(PathBuf::from("/Users/me/claude")),
                    cmd: Cmd::agent(Agent::Claude, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
            )]),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (
                    1,
                    vec![
                        plugin_pane(8),
                        terminal_pane_with_command(43, false, "codex"),
                        terminal_pane_with_command(44, true, "/bin/zsh"),
                    ],
                ),
            ]),
            |pane_id| match pane_id {
                43 => Some(PathBuf::from("/Users/me/codex")),
                44 => Some(PathBuf::from("/Users/me/focused")),
                _ => None,
            },
        );

        pretty_assertions::assert_eq!(events, vec![]);
    }

    #[test]
    fn test_apply_pane_update_remote_inactive_tab_skips_agent_cwd_for_non_agent_snapshot() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            other_tabs: HashMap::from([(
                8,
                StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(44),
                    cwd: Some(PathBuf::from("/Users/me/focused")),
                    cmd: Cmd::Running("zsh".to_string()),
                    indicator: TabIndicator::NoAgent,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
            )]),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (
                    1,
                    vec![
                        plugin_pane(8),
                        terminal_pane_with_command(43, false, "codex"),
                        terminal_pane_with_command(44, true, "/bin/zsh"),
                    ],
                ),
            ]),
            |pane_id| match pane_id {
                43 => Some(PathBuf::from("/Users/me/codex")),
                44 => Some(PathBuf::from("/Users/me/focused")),
                _ => None,
            },
        );

        pretty_assertions::assert_eq!(events, vec![]);
    }

    #[test]
    fn test_apply_pane_update_remote_inactive_tab_skips_cwd_for_different_running_command_snapshot() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            other_tabs: HashMap::from([(
                8,
                StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(45),
                    cwd: Some(PathBuf::from("/Users/me/gkg")),
                    cmd: Cmd::Running("gkg".to_string()),
                    indicator: TabIndicator::NoAgent,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
            )]),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (1, vec![plugin_pane(8), terminal_pane_with_command(43, true, "cargo")]),
            ]),
            |pane_id| (pane_id == 43).then(|| PathBuf::from("/Users/me/cargo")),
        );

        pretty_assertions::assert_eq!(events, vec![]);
    }

    #[test]
    fn test_apply_pane_update_hydrates_remote_tab_from_held_manifest_pane() {
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #4"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            sync_requested: true,
            cwds_by_pane: HashMap::from([(43, PathBuf::from("/Users/me/project"))]),
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (
                    1,
                    vec![
                        plugin_pane(8),
                        PaneInfo {
                            is_held: true,
                            ..terminal_pane_with_title(43, false, "gkg")
                        },
                    ],
                ),
            ]),
            noop_pane_cwd,
        );
        pretty_assertions::assert_eq!(
            events,
            vec![Event::RemoteTabUpdated {
                source_plugin_id: 8,
                snapshot: StateSnapshotPayload {
                    tab_id: 20,
                    seq: 0,
                    focused_pane_id: Some(43),
                    cwd: Some(PathBuf::from("/Users/me/project")),
                    cmd: Cmd::Running("gkg".to_string()),
                    indicator: TabIndicator::NoAgent,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
                evict_ids: vec![],
            }]
        );

        let _ = state.apply_all(&events);
        assert2::assert!(let Some(row) = state.frame.get(1));
        pretty_assertions::assert_eq!(row.path_label, "~/project");
        pretty_assertions::assert_eq!(row.cmd, Cmd::Running("gkg".to_string()));
    }

    #[test]
    fn test_apply_pane_update_updates_remote_tab_when_only_focus_changes() {
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            other_tabs: HashMap::from([(
                8,
                StateSnapshotPayload {
                    tab_id: 20,
                    seq: 0,
                    focused_pane_id: Some(43),
                    cwd: Some(PathBuf::from("/Users/me/project")),
                    cmd: Cmd::agent(Agent::Codex, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
            )]),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (
                    1,
                    vec![
                        plugin_pane(8),
                        terminal_pane_with_command(43, false, "codex"),
                        terminal_pane_with_command(44, true, "codex"),
                    ],
                ),
            ]),
            |pane_id| (pane_id == 44).then(|| PathBuf::from("/Users/me/project")),
        );
        pretty_assertions::assert_eq!(
            events,
            vec![Event::RemoteTabUpdated {
                source_plugin_id: 8,
                snapshot: StateSnapshotPayload {
                    tab_id: 20,
                    seq: 0,
                    focused_pane_id: Some(44),
                    cwd: Some(PathBuf::from("/Users/me/project")),
                    cmd: Cmd::agent(Agent::Codex, AgentState::Acknowledged),
                    indicator: TabIndicator::Seen,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
                evict_ids: vec![],
            }]
        );

        let _ = state.apply_all(&events);
        pretty_assertions::assert_eq!(
            state.other_tabs.get(&8).and_then(|snapshot| snapshot.focused_pane_id),
            Some(44)
        );
    }

    #[test]
    fn test_apply_pane_update_repairs_existing_remote_cwd_from_manifest() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![
                TabInfo {
                    active: true,
                    ..tab_with_name(10, 0, "Tab #1")
                },
                tab_with_name(20, 1, "Tab #2"),
            ],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                cwd: Some(PathBuf::from("/Users/me/current")),
                ..CurrentTab::new(10)
            }),
            other_tabs: HashMap::from([(
                8,
                StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(43),
                    cwd: None,
                    cmd: Cmd::agent(Agent::Codex, AgentState::Busy),
                    indicator: TabIndicator::Busy,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
            )]),
            sync_requested: true,
            home_dir: PathBuf::from("/Users/me"),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![
                (
                    0,
                    vec![plugin_pane(7), terminal_pane_with_command(42, true, "/bin/zsh")],
                ),
                (1, vec![plugin_pane(8), terminal_pane_with_command(43, false, "codex")]),
            ]),
            |pane_id| (pane_id == 43).then(|| PathBuf::from("/Users/me/project")),
        );

        pretty_assertions::assert_eq!(
            events,
            vec![Event::RemoteTabUpdated {
                source_plugin_id: 8,
                snapshot: StateSnapshotPayload {
                    tab_id: 20,
                    seq: 7,
                    focused_pane_id: Some(43),
                    cwd: Some(PathBuf::from("/Users/me/project")),
                    cmd: Cmd::agent(Agent::Codex, AgentState::Busy),
                    indicator: TabIndicator::Busy,
                    git_stat: GitStat::default(),
                    pane_agents: vec![],
                },
                evict_ids: vec![],
            }]
        );
    }

    #[test]
    fn test_apply_pane_update_active_tab_without_focused_pane_does_not_fake_focus() {
        let state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: vec![TabInfo {
                active: true,
                ..tab_with_name(10, 0, "Tab #1")
            }],
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };

        let events = derive(
            &state,
            &manifest(vec![(
                0,
                vec![plugin_pane(7), terminal_pane_with_command(42, false, "codex")],
            )]),
            |pane_id| (pane_id == 42).then(|| PathBuf::from("/Users/me/project")),
        );
        pretty_assertions::assert_eq!(
            events,
            vec![
                Event::PanesChanged {
                    observed_pane_ids: std::iter::once(42).collect(),
                    retained_pane_ids: std::iter::once(42).collect(),
                },
                Event::SyncRequested,
            ]
        );
    }

    #[test]
    fn test_partial_manifest_does_not_drop_running_agent_on_first_miss() {
        let mut state = TbarState {
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
        pretty_assertions::assert_eq!(
            partial_events,
            vec![
                Event::PanesChanged {
                    observed_pane_ids: std::iter::once(43).collect(),
                    retained_pane_ids: [42, 43].into_iter().collect(),
                },
                Event::SyncRequested,
            ]
        );

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.pane_ids, HashSet::from([42, 43]));
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Busy);
        pretty_assertions::assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Codex, AgentState::Busy));

        state.sync_frame();
        let frame = &state.frame;
        assert2::assert!(let Some(row) = frame.first());
        pretty_assertions::assert_eq!(row.cmd, Cmd::agent(Agent::Claude, AgentState::Busy));
        pretty_assertions::assert_eq!(row.indicator, TabIndicator::Busy);
    }

    #[test]
    fn test_partial_manifest_drops_missing_running_agent_after_second_miss() {
        let mut state = TbarState {
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
        pretty_assertions::assert_eq!(
            partial_events,
            vec![
                Event::AgentLost { pane_id: 42 },
                Event::PanesChanged {
                    observed_pane_ids: std::iter::once(43).collect(),
                    retained_pane_ids: std::iter::once(43).collect(),
                },
            ]
        );

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.pane_ids, HashSet::from([43]));
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Busy);
        pretty_assertions::assert_eq!(current_tab.display_cmd(), Cmd::agent(Agent::Claude, AgentState::Busy));
    }

    #[test]
    fn test_pane_update_ignores_stale_title_when_command_is_shell() {
        let mut state = TbarState {
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

        let _ = state.apply_all(&[Event::AgentLost { pane_id: 42 }]);
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

        let events = derive(&state, &manifest, noop_pane_cwd);

        pretty_assertions::assert_eq!(events, vec![]);
    }

    #[test]
    fn test_pane_update_clears_tracked_agent_when_process_changes() {
        let state = TbarState {
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
        let events = derive(&state, &manifest, noop_pane_cwd);
        pretty_assertions::assert_eq!(
            events,
            vec![
                Event::FocusChanged {
                    new_pane: Some(FocusedPane { id: 42, label: None }),
                    acknowledge_existing_attention: false,
                },
                Event::AgentLost { pane_id: 42 },
                Event::SyncRequested,
            ]
        );
    }

    #[test]
    fn test_pane_update_clears_unfocused_tracked_agent_when_process_changes() {
        let state = TbarState {
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
        let events = derive(&state, &manifest, noop_pane_cwd);
        pretty_assertions::assert_eq!(events, vec![Event::AgentLost { pane_id: 42 }, Event::SyncRequested,]);
    }

    #[test]
    fn test_apply_pane_update_keeps_idle_agent_when_title_becomes_path() {
        let mut state = TbarState {
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
        pretty_assertions::assert_eq!(
            events,
            vec![
                Event::FocusChanged {
                    new_pane: Some(FocusedPane { id: 42, label: None }),
                    acknowledge_existing_attention: false,
                },
                Event::SyncRequested,
            ]
        );

        assert2::assert!(let Some(current_tab) = state.current_tab.as_ref());
        pretty_assertions::assert_eq!(current_tab.tab_indicator(), TabIndicator::Seen);
        pretty_assertions::assert_eq!(
            current_tab.display_cmd(),
            Cmd::agent(Agent::Codex, AgentState::Acknowledged)
        );
    }
}
