use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;

use agg::Cmd;
use agg::GitStat;
use agg::ParseError;
use agg::TabIndicator;
use agg::TabStateEntry;
use ytil_agents::agent::Agent;
use ytil_agents::agent::AgentIcon;
use zellij_tile::prelude::EventType;
use zellij_tile::prelude::MessageToPlugin;
use zellij_tile::prelude::PaneId;
use zellij_tile::prelude::PaneInfo;
use zellij_tile::prelude::PaneManifest;
use zellij_tile::prelude::PipeMessage;
use zellij_tile::prelude::SessionInfo;
use zellij_tile::prelude::TabInfo;

use crate::plugin::nudge::Nudge;
use crate::plugin::pane::FocusedPane;
use crate::plugin::tbar::current_tab::AgentLossSource;
use crate::plugin::tbar::current_tab::AgentPaneSource;
use crate::plugin::tbar::current_tab::CurrentTab;
use crate::plugin::tbar::events::PipeEvent;
use crate::plugin::tbar::events::PipeEventError;
use crate::plugin::tbar::ui::TabRow;

pub mod current_tab;
pub mod events;
pub mod events_from;
mod frame;
mod queries;
mod state_transitions;
mod tabs;
pub mod ui;

pub const AGG_SYNC_PIPE: &str = "agg-sync";
const CONTEXT_KEY_GIT_STAT: &str = "git-stat";
const TAB_STATE_ENTRY_LINES: usize = 9;

#[derive(Default)]
pub struct TbarState {
    pub plugin_id: u32,
    pub all_tabs: Vec<TabInfo>,
    pub current_tab: Option<CurrentTab>,
    pub other_tabs: HashMap<u32, StateSnapshotPayload>,
    pub cwds_by_pane: HashMap<u32, PathBuf>,
    pub known_active_tab_id: Option<usize>,
    pub sync_requested: bool,
    pub nudged_pane_ids: HashSet<u32>,
    pub home_dir: PathBuf,
    pub zellij_session_name: Option<String>,
    pub frame: Vec<TabRow>,
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug)]
pub struct StateSnapshotPayload {
    pub tab_id: usize,
    pub seq: u64,
    pub focused_pane_id: Option<u32>,
    pub cwd_pane_id: Option<u32>,
    pub cwd: Option<PathBuf>,
    pub cmd: Cmd,
    pub indicator: TabIndicator,
    pub git_stat: GitStat,
    pub pane_agents: Vec<PaneAgentSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneAgentSnapshot {
    pub pane_id: u32,
    pub agent: Agent,
    pub indicator: TabIndicator,
}

impl StateSnapshotPayload {
    pub fn from_current_tab(value: &CurrentTab, cwds_by_pane: &HashMap<u32, PathBuf>) -> Self {
        let mut pane_agents = value
            .pane_state_by_pane
            .iter()
            .map(|(&pane_id, pane_state)| {
                let cmd = pane_state.cmd();
                PaneAgentSnapshot {
                    pane_id,
                    agent: pane_state.agent,
                    indicator: TabIndicator::from_cmd(&cmd),
                }
            })
            .collect::<Vec<_>>();
        pane_agents.sort_by_key(|pane| pane.pane_id);
        let display = value.current_row_display_source(false, cwds_by_pane);

        Self {
            tab_id: value.tab_id,
            seq: value.seq,
            focused_pane_id: value
                .active_focus_pane_id
                .or_else(|| value.focused_pane.as_ref().map(|focused_pane| focused_pane.id)),
            cwd_pane_id: display.pane_id,
            cwd: display.cwd,
            cmd: display.cmd,
            indicator: display.indicator,
            git_stat: display.git_stat,
            pane_agents,
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
        let focused_pane_id = value
            .args
            .get("focused_pane_id")
            .map(|v| {
                v.parse::<u32>().map_err(|_| {
                    PipeEventError::Parse(ParseError::Invalid {
                        field: "focused_pane_id",
                        value: v.clone(),
                    })
                })
            })
            .transpose()?;
        let cwd_pane_id = value
            .args
            .get("cwd_pane_id")
            .map(|v| {
                v.parse::<u32>().map_err(|_| {
                    PipeEventError::Parse(ParseError::Invalid {
                        field: "cwd_pane_id",
                        value: v.clone(),
                    })
                })
            })
            .transpose()?;
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
            focused_pane_id,
            cwd_pane_id,
            cwd: entry.cwd,
            cmd: entry.cmd,
            indicator: entry.indicator,
            git_stat: entry.git_stat,
            pane_agents: parse_pane_agent_snapshots(payload)?,
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
            git_stat: value.git_stat.clone(),
        };
        let mut args = BTreeMap::new();
        args.insert("type".to_string(), "state_snapshot".to_string());
        args.insert("tab_id".to_string(), value.tab_id.to_string());
        args.insert("seq".to_string(), value.seq.to_string());
        if let Some(focused_pane_id) = value.focused_pane_id {
            args.insert("focused_pane_id".to_string(), focused_pane_id.to_string());
        }
        if let Some(cwd_pane_id) = value.cwd_pane_id {
            args.insert("cwd_pane_id".to_string(), cwd_pane_id.to_string());
        }
        let mut payload = entry.to_string();
        for pane in &value.pane_agents {
            let _ = writeln!(
                payload,
                "{}\t{}\t{}",
                pane.pane_id,
                pane.agent.name(),
                pane.indicator.as_str()
            );
        }
        Self::new(AGG_SYNC_PIPE.to_string())
            .with_args(args)
            .with_payload(payload)
    }
}

fn parse_pane_agent_snapshots(payload: &str) -> Result<Vec<PaneAgentSnapshot>, PipeEventError> {
    payload
        .lines()
        .skip(TAB_STATE_ENTRY_LINES)
        .filter(|line| !line.is_empty())
        .map(parse_pane_agent_snapshot)
        .collect::<Result<Vec<_>, _>>()
        .map_err(PipeEventError::Parse)
}

fn parse_pane_agent_snapshot(line: &str) -> Result<PaneAgentSnapshot, ParseError> {
    let mut fields = line.split('\t');
    let pane_id = fields
        .next()
        .ok_or(ParseError::Missing("pane_agent pane_id"))
        .and_then(|value| {
            value.parse::<u32>().map_err(|_| ParseError::Invalid {
                field: "pane_agent pane_id",
                value: value.to_string(),
            })
        })?;
    let agent = fields
        .next()
        .ok_or(ParseError::Missing("pane_agent agent"))
        .and_then(Agent::from_name)?;
    let indicator = fields
        .next()
        .ok_or(ParseError::Missing("pane_agent indicator"))
        .and_then(TabIndicator::parse)?;
    if fields.next().is_some() {
        return Err(ParseError::Invalid {
            field: "pane_agent",
            value: line.to_string(),
        });
    }

    Ok(PaneAgentSnapshot {
        pane_id,
        agent,
        indicator,
    })
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum Event {
    TabCreated {
        tab_id: usize,
    },
    TabRemapped {
        new_tab_id: usize,
    },
    PanesChanged {
        observed_pane_ids: HashSet<u32>,
        retained_pane_ids: HashSet<u32>,
    },
    FocusChanged {
        new_pane: Option<FocusedPane>,
        acknowledge_existing_attention: bool,
    },
    CwdChanged {
        pane_id: u32,
        new_cwd: PathBuf,
    },
    AgentDetected {
        pane_id: u32,
        agent: Agent,
        source: AgentPaneSource,
    },
    AgentBusy {
        pane_id: u32,
        agent: Agent,
    },
    AgentIdle {
        pane_id: u32,
        agent: Agent,
    },
    AgentLost {
        pane_id: u32,
        source: AgentLossSource,
    },
    GitStatChanged {
        new_stat: GitStat,
    },
    RemoteTabUpdated {
        source_plugin_id: u32,
        snapshot: StateSnapshotPayload,
        evict_ids: Vec<u32>,
    },
    ActiveTabChanged {
        active_tab_id: usize,
    },
    TopologyChanged,
    BecameActive,
    AllTabsReplaced {
        new_tabs: Vec<TabInfo>,
    },
    SyncRequested,
}

pub fn load(state: &mut TbarState, home_dir: PathBuf) {
    state.plugin_id = zellij_tile::prelude::get_plugin_ids().plugin_id;
    state.home_dir = home_dir;
}

pub fn update_permission_granted(state: &mut TbarState) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let mut env_vars = zellij_tile::prelude::get_session_environment_variables();
        state.zellij_session_name = env_vars
            .remove("ZELLIJ_SESSION_NAME")
            .filter(|session| !session.is_empty());
    }
    zellij_tile::prelude::subscribe(&[
        EventType::TabUpdate,
        EventType::SessionUpdate,
        EventType::PaneUpdate,
        EventType::PaneClosed,
        EventType::CwdChanged,
        EventType::Mouse,
        EventType::RunCommandResult,
    ]);
    zellij_tile::prelude::set_selectable(false);
    state.sync_frame()
}

pub fn render(state: &TbarState, rows: usize, cols: usize, buf: &mut String) {
    crate::plugin::tbar::ui::render_frame(&state.frame, rows, cols, buf);
}

pub fn update_tabs(state: &mut TbarState, mut tabs: Vec<TabInfo>) -> bool {
    let active_tab_id = tabs.iter().find(|tab| tab.active).map(|tab| tab.tab_id);
    let landing_focus = active_tab_id
        .and_then(|active_tab_id| resolve_active_tab_landing_focus(active_tab_id, &tabs, state.current_tab.as_ref()));
    update_host_tabs(state, &mut tabs, active_tab_id, landing_focus, true)
}

pub fn update_sessions(state: &mut TbarState, sessions: &[SessionInfo]) -> bool {
    let Some(session) = sessions.iter().find(|session| session.is_current_session) else {
        return false;
    };
    let mut tabs = session.tabs.clone();
    tabs.sort_by_key(|tab| tab.position);
    let active_tab_id = tabs.iter().find(|tab| tab.active).map(|tab| tab.tab_id);
    // SessionUpdate is host-authoritative. For same-topology updates, copy only
    // the active bit so first focus into an unhydrated tab can move the rail
    // without replaying stale tab metadata. For topology changes, consume the
    // full host tab list because a closed tab can disappear before TabUpdate.
    if !crate::plugin::tbar::tabs::topology_changed(&state.all_tabs, &tabs) {
        let Some(active_tab_id) = active_tab_id else {
            return false;
        };
        let mut active_tabs = state.all_tabs.clone();
        for tab in &mut active_tabs {
            tab.active = tab.tab_id == active_tab_id;
        }
        return update_host_tabs(state, &mut active_tabs, Some(active_tab_id), None, false);
    }
    update_host_tabs(state, &mut tabs, active_tab_id, None, false)
}

fn update_host_tabs(
    state: &mut TbarState,
    tabs: &mut [TabInfo],
    active_tab_id: Option<usize>,
    landing_focus: Option<FocusedPane>,
    broadcast: bool,
) -> bool {
    tabs.sort_by_key(|tab| tab.position);
    if state.all_tabs == tabs {
        return false;
    }
    let events = crate::plugin::tbar::events_from::tab_update::derive(state, tabs, landing_focus);
    let cwd_changed = cache_focus_cwds_for_events(state, &events);
    let frame_changed = state.apply_all(&events);
    if broadcast {
        send_tab_topology(tabs);
    }
    if broadcast && let Some(active_tab_id) = active_tab_id {
        send_active_tab(active_tab_id);
    }
    handle_events(state, &events);
    cwd_changed || frame_changed || !events.is_empty()
}

pub fn update_panes(state: &mut TbarState, manifest: &PaneManifest) -> bool {
    let events = crate::plugin::tbar::events_from::pane_update::derive(state, manifest, |pane_id| {
        zellij_tile::prelude::get_pane_cwd(PaneId::Terminal(pane_id)).ok()
    });
    apply_and_handle_events(state, &events)
}

pub fn update_pane_closed(state: &mut TbarState, pane_id: u32) -> bool {
    let events = crate::plugin::tbar::events_from::pane_close::derive(state, pane_id);
    apply_and_handle_events(state, &events)
}

pub fn update_cwd(state: &mut TbarState, pane_id: u32, cwd: PathBuf) -> bool {
    let events = crate::plugin::tbar::events_from::cwd::derive(state, pane_id, cwd);
    apply_and_handle_events(state, &events)
}

pub fn update_run_command_result(
    state: &mut TbarState,
    exit_code: Option<i32>,
    stdout: &[u8],
    context: &BTreeMap<String, String>,
) -> bool {
    let Some(requested_cwd) = context.get(CONTEXT_KEY_GIT_STAT).map(PathBuf::from) else {
        return false;
    };
    let events = crate::plugin::tbar::events_from::run_command::derive(state, &requested_cwd, exit_code, stdout);
    apply_and_handle_events(state, &events)
}

pub fn update_mouse_left_click(state: &TbarState, row: isize, last_cols: usize) -> bool {
    let Ok(row) = usize::try_from(row) else {
        return false;
    };
    let content_w = last_cols.saturating_sub(1);
    if let Some(tab_idx) = crate::plugin::tbar::ui::tab_index_at_row(&state.frame, row, content_w)
        && let Some(tab) = state.all_tabs.get(tab_idx)
        && let Ok(pos) = u32::try_from(tab.position)
    {
        zellij_tile::prelude::switch_tab_to(pos.saturating_add(1));
    }
    false
}

pub fn pipe(state: &mut TbarState, pipe_message: &PipeMessage) -> bool {
    let event = match PipeEvent::try_from(pipe_message) {
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
            if requester_plugin_id == state.plugin_id {
                return false;
            }
            send_current_tab_snapshot(state, Some(requester_plugin_id));
            false
        }
        PipeEvent::ActiveTab { active_tab_id } => {
            let landing_focus =
                resolve_active_tab_landing_focus(active_tab_id, &state.all_tabs, state.current_tab.as_ref());
            let events = crate::plugin::tbar::events_from::active_tab::derive(state, active_tab_id, landing_focus);
            let cwd_changed = cache_focus_cwds_for_events(state, &events);
            let frame_changed = state.apply_all(&events);
            handle_events(state, &events);
            cwd_changed || frame_changed || !events.is_empty()
        }
        PipeEvent::TabTopology { source_plugin_id, tabs } => {
            if source_plugin_id == state.plugin_id {
                return false;
            }
            apply_synced_tab_topology(state, tabs)
        }
        PipeEvent::StateSnapshot {
            source_plugin_id,
            snapshot,
        } => {
            let events = crate::plugin::tbar::events_from::snapshot::derive(state, source_plugin_id, &snapshot);
            let frame_changed = state.apply_all(&events);
            frame_changed || !events.is_empty()
        }
        PipeEvent::Agent(agent_event) => {
            // Agent pipes are broadcast across tbars; hydrate cwd only for panes owned by this plugin.
            let is_current_pane = state
                .current_tab
                .as_ref()
                .is_some_and(|current_tab| current_tab.pane_ids.contains(&agent_event.pane_id));
            let cwd_changed =
                if matches!(agent_event.kind, ytil_agents::agent::AgentEventKind::Exit) || !is_current_pane {
                    false
                } else {
                    cache_pane_cwd(state, agent_event.pane_id)
                };
            let events = crate::plugin::tbar::events_from::agent::derive(state, &agent_event);
            let frame_changed = state.apply_all(&events);
            handle_events(state, &events);
            if cwd_changed && events.is_empty() {
                run_current_tab_git_stat(state);
                send_current_tab_snapshot(state, None);
            }
            cwd_changed || frame_changed || !events.is_empty()
        }
    }
}

fn cache_pane_cwd(state: &mut TbarState, pane_id: u32) -> bool {
    let Ok(cwd) = zellij_tile::prelude::get_pane_cwd(PaneId::Terminal(pane_id)) else {
        eprintln!("agg: missing cwd for terminal pane {pane_id}");
        return false;
    };
    if state.cwds_by_pane.get(&pane_id) == Some(&cwd) {
        return false;
    }
    state.cwds_by_pane.insert(pane_id, cwd);
    true
}

fn cache_focus_cwds_for_events(state: &mut TbarState, events: &[Event]) -> bool {
    let mut changed = false;
    for pane_id in events.iter().filter_map(|event| match event {
        Event::FocusChanged {
            new_pane: Some(pane), ..
        } => Some(pane.id),
        Event::TabCreated { .. }
        | Event::TabRemapped { .. }
        | Event::PanesChanged { .. }
        | Event::FocusChanged { new_pane: None, .. }
        | Event::CwdChanged { .. }
        | Event::AgentDetected { .. }
        | Event::AgentBusy { .. }
        | Event::AgentIdle { .. }
        | Event::AgentLost { .. }
        | Event::GitStatChanged { .. }
        | Event::RemoteTabUpdated { .. }
        | Event::ActiveTabChanged { .. }
        | Event::TopologyChanged
        | Event::BecameActive
        | Event::AllTabsReplaced { .. }
        | Event::SyncRequested => None,
    }) {
        changed |= cache_pane_cwd(state, pane_id);
    }
    changed
}

fn apply_synced_tab_topology(state: &mut TbarState, mut tabs: Vec<TabInfo>) -> bool {
    tabs.sort_by_key(|tab| tab.position);
    // tab_topology is sent by peer agg instances and may arrive late. Use it
    // only to bootstrap empty state; after host topology exists, accepting it
    // can replay stale active flags or reintroduce a tab removed by SessionUpdate.
    if !state.all_tabs.is_empty() {
        return false;
    }
    if state.all_tabs == tabs {
        return false;
    }
    state.apply_all(&[Event::AllTabsReplaced { new_tabs: tabs }])
}

fn apply_and_handle_events(state: &mut TbarState, events: &[Event]) -> bool {
    let cwd_changed = cache_focus_cwds_for_events(state, events);
    let frame_changed = state.apply_all(events);
    handle_events(state, events);
    cwd_changed || frame_changed || !events.is_empty()
}

fn handle_events(state: &mut TbarState, events: &[Event]) {
    for event in events {
        match event {
            Event::GitStatChanged { .. } => {
                // Git-stat results refresh current state; rerunning here would make same-path updates self-suppress.
                send_current_tab_snapshot(state, None);
            }
            Event::TabCreated { .. }
            | Event::TabRemapped { .. }
            | Event::AgentDetected { .. }
            | Event::AgentBusy { .. }
            | Event::AgentLost { .. }
            | Event::ActiveTabChanged { .. }
            | Event::BecameActive
            | Event::AgentIdle { .. }
            | Event::FocusChanged { .. }
            | Event::CwdChanged { .. } => {
                run_current_tab_git_stat(state);
                send_current_tab_snapshot(state, None);
            }
            Event::SyncRequested => send_sync_request(),
            Event::PanesChanged { .. }
            | Event::RemoteTabUpdated { .. }
            | Event::TopologyChanged
            | Event::AllTabsReplaced { .. } => {}
        }
    }
    for (pane_id, nudge) in state.nudges() {
        state.mark_nudged(pane_id);
        send_nudge(&state.home_dir, state.zellij_session_name.as_deref(), &nudge);
    }
}

fn send_nudge(home_dir: &Path, session: Option<&str>, nudge: &Nudge) {
    let body = nudge.agent.to_string();
    let icon_path = AgentIcon::from(nudge.agent).path(home_dir);
    let icon_path = icon_path.to_string_lossy();
    let tab_id = nudge.tab_id.to_string();
    let pane_id = nudge.pane_id.to_string();
    let mut args = vec![
        "agg",
        "nudge",
        nudge.path.as_str(),
        body.as_str(),
        tab_id.as_str(),
        pane_id.as_str(),
        icon_path.as_ref(),
    ];
    if let Some(session) = session {
        args.push("--session");
        args.push(session);
    }
    zellij_tile::prelude::run_command(&args, BTreeMap::new());
}

fn send_sync_request() {
    let mut args = BTreeMap::new();
    args.insert("type".to_string(), "sync_request".to_string());
    zellij_tile::prelude::pipe_message_to_plugin(MessageToPlugin::new(AGG_SYNC_PIPE.to_string()).with_args(args));
}

fn send_active_tab(active_tab_id: usize) {
    let mut args = BTreeMap::new();
    args.insert("type".to_string(), "active_tab".to_string());
    args.insert("tab_id".to_string(), active_tab_id.to_string());
    zellij_tile::prelude::pipe_message_to_plugin(MessageToPlugin::new(AGG_SYNC_PIPE.to_string()).with_args(args));
}

fn send_tab_topology(tabs: &[TabInfo]) {
    let Ok(payload) = serde_json::to_string(tabs).inspect_err(|error| eprintln!("agg: {error}")) else {
        return;
    };
    let mut args = BTreeMap::new();
    args.insert("type".to_string(), "tab_topology".to_string());
    zellij_tile::prelude::pipe_message_to_plugin(
        MessageToPlugin::new(AGG_SYNC_PIPE.to_string())
            .with_args(args)
            .with_payload(payload),
    );
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
        || zellij_tile::prelude::get_focused_pane_info().ok(),
        zellij_tile::prelude::get_pane_info,
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
        && let Some(focused_pane) = crate::plugin::pane::focused_pane_from_pane_info(&pane)
    {
        return Some(focused_pane);
    }

    let current_tab = current_tab?;
    current_tab.pane_ids.iter().find_map(|pane_id| {
        let pane = get_pane_info(PaneId::Terminal(*pane_id))?;
        if !pane.is_focused {
            return None;
        }
        crate::plugin::pane::focused_pane_from_pane_info(&pane)
    })
}

fn send_current_tab_snapshot(state: &TbarState, target_plugin_id: Option<u32>) {
    let Some(current_tab) = state.current_tab.as_ref() else {
        return;
    };
    let mut message = MessageToPlugin::from(&StateSnapshotPayload::from_current_tab(
        current_tab,
        &state.cwds_by_pane,
    ));
    if let Some(target_plugin_id) = target_plugin_id {
        message = message.with_destination_plugin_id(target_plugin_id);
    }
    zellij_tile::prelude::pipe_message_to_plugin(message);
}

fn run_current_tab_git_stat(state: &TbarState) {
    let Some(current_tab) = state.current_tab.as_ref() else {
        return;
    };
    let display = current_tab.current_row_display_source(state.current_tab_is_active(), &state.cwds_by_pane);
    let Some(cwd) = display.cwd else {
        if let Some(pane_id) = display.pane_id {
            eprintln!("agg: missing display cwd for terminal pane {pane_id}");
        }
        return;
    };
    let cwd_str = cwd.display().to_string();
    let mut context = BTreeMap::new();
    context.insert(CONTEXT_KEY_GIT_STAT.into(), cwd_str.clone());
    let args: Vec<&str> = vec!["agg", "git-stat", "tbar", &cwd_str];
    zellij_tile::prelude::run_command_with_env_variables_and_cwd(&args, BTreeMap::new(), cwd, context);
}

#[cfg(test)]
pub mod test_support {
    use std::path::PathBuf;

    use agg::Cmd;
    use agg::GitStat;
    use agg::TabIndicator;
    use ytil_agents::agent::Agent;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;
    use zellij_tile::prelude::TabInfo;

    use crate::plugin::tbar::Event;
    use crate::plugin::tbar::StateSnapshotPayload;
    use crate::plugin::tbar::TbarState;
    use crate::plugin::tbar::current_tab::AgentPanePhase;
    use crate::plugin::tbar::current_tab::AgentPaneSource;
    use crate::plugin::tbar::current_tab::AgentPaneState;
    use crate::plugin::tbar::current_tab::PaneFocus;

    pub const fn noop_pane_cwd(_pane_id: u32) -> Option<PathBuf> {
        None
    }

    pub fn tab_with_name(tab_id: usize, position: usize, name: &str) -> TabInfo {
        TabInfo {
            tab_id,
            position,
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn plugin_pane(id: u32) -> PaneInfo {
        PaneInfo {
            id,
            is_plugin: true,
            ..Default::default()
        }
    }

    pub fn terminal_pane_with_command(id: u32, is_focused: bool, command: &str) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            terminal_command: Some(command.to_string()),
            ..Default::default()
        }
    }

    pub fn terminal_pane_with_title(id: u32, is_focused: bool, title: &str) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            title: title.to_string(),
            ..Default::default()
        }
    }

    pub fn manifest(entries: Vec<(usize, Vec<PaneInfo>)>) -> PaneManifest {
        PaneManifest {
            panes: entries.into_iter().collect(),
        }
    }

    pub const fn pane_state(agent: Agent, phase: AgentPanePhase, focus: PaneFocus, phase_seq: u64) -> AgentPaneState {
        AgentPaneState {
            agent,
            phase,
            focus,
            source: AgentPaneSource::Manifest,
            phase_seq,
        }
    }

    pub fn snapshot(tab_id: usize, seq: u64, cmd: Cmd, indicator: TabIndicator) -> StateSnapshotPayload {
        StateSnapshotPayload {
            tab_id,
            seq,
            focused_pane_id: None,
            cwd_pane_id: None,
            cwd: None,
            cmd,
            indicator,
            git_stat: GitStat::default(),
            pane_agents: vec![],
        }
    }

    pub fn apply_pane_update(state: &mut TbarState, manifest: &PaneManifest) -> Vec<Event> {
        let events = crate::plugin::tbar::events_from::pane_update::derive(state, manifest, noop_pane_cwd);
        let _ = state.apply_all(&events);
        events
    }
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
    use zellij_tile::prelude::PaneId;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PipeMessage;
    use zellij_tile::prelude::PipeSource;
    use zellij_tile::prelude::SessionInfo;
    use zellij_tile::prelude::TabInfo;

    use crate::plugin::pane::FocusedPane;
    use crate::plugin::pane::FocusedPaneLabel;
    use crate::plugin::tbar::AGG_SYNC_PIPE;
    use crate::plugin::tbar::PaneAgentSnapshot;
    use crate::plugin::tbar::StateSnapshotPayload;
    use crate::plugin::tbar::TbarState;
    use crate::plugin::tbar::current_tab::AgentPanePhase;
    use crate::plugin::tbar::current_tab::CurrentTab;
    use crate::plugin::tbar::current_tab::PaneFocus;
    use crate::plugin::tbar::events::PipeEvent;
    use crate::plugin::tbar::events::PipeEventError;
    use crate::plugin::tbar::test_support::pane_state;

    #[test]
    fn test_sync_request_from_cli_fails_without_source_plugin_id() {
        let mut args = std::collections::BTreeMap::new();
        args.insert("type".to_string(), "sync_request".to_string());
        let msg = PipeMessage {
            source: PipeSource::Cli("x".to_string()),
            name: AGG_SYNC_PIPE.to_string(),
            payload: None,
            args,
            is_private: false,
        };
        let parsed = PipeEvent::try_from(&msg);

        assert2::assert!(
            let Err(PipeEventError::Parse(agg::ParseError::Missing("source"))) = parsed
        );
    }

    #[test]
    fn test_state_snapshot_from_current_tab_includes_sorted_pane_agent_states() {
        let current_tab = CurrentTab {
            pane_state_by_pane: HashMap::from([
                (
                    43,
                    pane_state(Agent::Claude, AgentPanePhase::AttentionUnseen, PaneFocus::Unfocused, 2),
                ),
                (
                    42,
                    pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Focused, 1),
                ),
            ]),
            ..CurrentTab::new(10)
        };
        let cwds_by_pane = HashMap::from([(43, PathBuf::from("/Users/me/claude"))]);

        let snapshot = StateSnapshotPayload::from_current_tab(&current_tab, &cwds_by_pane);

        pretty_assertions::assert_eq!(
            snapshot.pane_agents,
            vec![
                PaneAgentSnapshot {
                    pane_id: 42,
                    agent: Agent::Codex,
                    indicator: TabIndicator::Busy,
                },
                PaneAgentSnapshot {
                    pane_id: 43,
                    agent: Agent::Claude,
                    indicator: TabIndicator::Unseen,
                },
            ]
        );
        pretty_assertions::assert_eq!(snapshot.cmd, Cmd::agent(Agent::Claude, AgentState::NeedsAttention));
        pretty_assertions::assert_eq!(snapshot.indicator, TabIndicator::Unseen);
        pretty_assertions::assert_eq!(snapshot.cwd_pane_id, Some(43));
        pretty_assertions::assert_eq!(snapshot.cwd, Some(PathBuf::from("/Users/me/claude")));
    }

    #[test]
    fn test_state_snapshot_from_active_mat_uses_inactive_agent_winner_not_focused_shell() {
        let current_tab = CurrentTab {
            pane_ids: [42, 43].into_iter().collect(),
            focused_pane: Some(FocusedPane { id: 43, label: None }),
            active_focus_pane_id: Some(43),
            cwd_pane_id: Some(43),
            cwd: Some(PathBuf::from("/Users/me/shell")),
            git_stat: GitStat {
                path: PathBuf::from("/Users/me/shell"),
                insertions: 9,
                ..GitStat::default()
            },
            pane_state_by_pane: HashMap::from([(
                42,
                pane_state(Agent::Codex, AgentPanePhase::Running, PaneFocus::Unfocused, 1),
            )]),
            ..CurrentTab::new(10)
        };
        let cwds_by_pane = HashMap::from([
            (42, PathBuf::from("/Users/me/codex")),
            (43, PathBuf::from("/Users/me/shell")),
        ]);

        let snapshot = StateSnapshotPayload::from_current_tab(&current_tab, &cwds_by_pane);

        pretty_assertions::assert_eq!(snapshot.cwd_pane_id, Some(42));
        pretty_assertions::assert_eq!(snapshot.cwd, Some(PathBuf::from("/Users/me/codex")));
        pretty_assertions::assert_eq!(snapshot.cmd, Cmd::agent(Agent::Codex, AgentState::Busy));
        pretty_assertions::assert_eq!(snapshot.indicator, TabIndicator::Busy);
        pretty_assertions::assert_eq!(snapshot.git_stat, GitStat::default());
    }

    #[test]
    fn test_pipe_tab_topology_bootstraps_empty_hidden_tab_state_without_rebroadcast() {
        let tabs = vec![
            TabInfo {
                tab_id: 20,
                position: 0,
                name: "new".to_string(),
                active: true,
                ..Default::default()
            },
            TabInfo {
                tab_id: 10,
                position: 1,
                name: "old".to_string(),
                active: false,
                ..Default::default()
            },
        ];
        let mut state = TbarState {
            plugin_id: 7,
            sync_requested: false,
            ..Default::default()
        };
        let msg = PipeMessage {
            source: PipeSource::Plugin(8),
            name: AGG_SYNC_PIPE.to_string(),
            payload: Some(serde_json::to_string(&tabs).unwrap()),
            args: std::collections::BTreeMap::from([(String::from("type"), String::from("tab_topology"))]),
            is_private: false,
        };

        assert!(crate::plugin::tbar::pipe(&mut state, &msg));
        pretty_assertions::assert_eq!(state.all_tabs, tabs);
        pretty_assertions::assert_eq!(state.known_active_tab_id, Some(20));
        assert!(!state.sync_requested);
    }

    #[test]
    fn test_session_update_replaces_closed_tab_and_prunes_remote_snapshot() {
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(30),
            all_tabs: vec![
                TabInfo {
                    tab_id: 10,
                    position: 0,
                    active: false,
                    ..Default::default()
                },
                TabInfo {
                    tab_id: 20,
                    position: 1,
                    active: false,
                    ..Default::default()
                },
                TabInfo {
                    tab_id: 30,
                    position: 2,
                    active: true,
                    ..Default::default()
                },
            ],
            other_tabs: HashMap::from([
                (
                    8,
                    crate::plugin::tbar::test_support::snapshot(20, 1, Cmd::None, TabIndicator::NoAgent),
                ),
                (
                    9,
                    crate::plugin::tbar::test_support::snapshot(30, 1, Cmd::None, TabIndicator::NoAgent),
                ),
            ]),
            ..Default::default()
        };
        let host_tabs = vec![
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

        assert!(crate::plugin::tbar::update_sessions(
            &mut state,
            &[SessionInfo {
                is_current_session: true,
                tabs: host_tabs.clone(),
                ..Default::default()
            }],
        ));

        pretty_assertions::assert_eq!(state.all_tabs, host_tabs);
        pretty_assertions::assert_eq!(state.known_active_tab_id, Some(20));
        pretty_assertions::assert_eq!(state.other_tabs.len(), 1);
        assert!(state.other_tabs.contains_key(&8));
    }

    #[test]
    fn test_session_update_preserves_same_topology_tab_metadata() {
        let host_tabs = vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                name: "canonical-first".to_string(),
                active: true,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                name: "canonical-second".to_string(),
                active: false,
                ..Default::default()
            },
        ];
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: host_tabs,
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let same_topology_focus_move = vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                name: "stale-first".to_string(),
                active: false,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                name: "stale-second".to_string(),
                active: true,
                ..Default::default()
            },
        ];

        assert!(crate::plugin::tbar::update_sessions(
            &mut state,
            &[SessionInfo {
                is_current_session: true,
                tabs: same_topology_focus_move,
                ..Default::default()
            }],
        ));

        let expected_tabs = vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                name: "canonical-first".to_string(),
                active: false,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                name: "canonical-second".to_string(),
                active: true,
                ..Default::default()
            },
        ];
        pretty_assertions::assert_eq!(state.all_tabs, expected_tabs);
        pretty_assertions::assert_eq!(state.known_active_tab_id, Some(20));
    }

    #[test]
    fn test_session_update_moves_active_rail_before_target_tab_hydrates() {
        let host_tabs = vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                active: true,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                active: false,
                ..Default::default()
            },
        ];
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(10),
            all_tabs: host_tabs,
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let first_focus_to_unhydrated_tab = vec![
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

        assert!(crate::plugin::tbar::update_sessions(
            &mut state,
            &[SessionInfo {
                is_current_session: true,
                tabs: first_focus_to_unhydrated_tab,
                ..Default::default()
            }],
        ));

        assert2::assert!(let Some(first) = state.frame.first());
        assert2::assert!(let Some(second) = state.frame.get(1));
        pretty_assertions::assert_eq!(first.active, false);
        pretty_assertions::assert_eq!(second.active, true);
        pretty_assertions::assert_eq!(state.known_active_tab_id, Some(20));
    }

    #[test]
    fn test_pipe_tab_topology_does_not_reintroduce_host_removed_tab() {
        let host_tabs = vec![
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
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(20),
            all_tabs: host_tabs.clone(),
            current_tab: Some(CurrentTab::new(10)),
            ..Default::default()
        };
        let stale_tabs = vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                active: false,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                active: false,
                ..Default::default()
            },
            TabInfo {
                tab_id: 30,
                position: 2,
                active: true,
                ..Default::default()
            },
        ];
        let msg = PipeMessage {
            source: PipeSource::Plugin(9),
            name: AGG_SYNC_PIPE.to_string(),
            payload: Some(serde_json::to_string(&stale_tabs).unwrap()),
            args: std::collections::BTreeMap::from([(String::from("type"), String::from("tab_topology"))]),
            is_private: false,
        };

        assert!(!crate::plugin::tbar::pipe(&mut state, &msg));

        pretty_assertions::assert_eq!(state.all_tabs, host_tabs);
        pretty_assertions::assert_eq!(state.known_active_tab_id, Some(20));
    }

    #[test]
    fn test_pipe_tab_topology_does_not_replay_active_tab_after_host_state_exists() {
        let host_tabs = vec![
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
        let mut state = TbarState {
            plugin_id: 7,
            known_active_tab_id: Some(20),
            all_tabs: host_tabs.clone(),
            ..Default::default()
        };
        let stale_active_tabs = vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                active: true,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                active: false,
                ..Default::default()
            },
        ];
        let msg = PipeMessage {
            source: PipeSource::Plugin(9),
            name: AGG_SYNC_PIPE.to_string(),
            payload: Some(serde_json::to_string(&stale_active_tabs).unwrap()),
            args: std::collections::BTreeMap::from([(String::from("type"), String::from("tab_topology"))]),
            is_private: false,
        };

        assert!(!crate::plugin::tbar::pipe(&mut state, &msg));

        pretty_assertions::assert_eq!(state.all_tabs, host_tabs);
        pretty_assertions::assert_eq!(state.known_active_tab_id, Some(20));
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

        let landing_focus = crate::plugin::tbar::resolve_active_tab_landing_focus_with(
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

        pretty_assertions::assert_eq!(
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

        let landing_focus = crate::plugin::tbar::resolve_active_tab_landing_focus_with(
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

        pretty_assertions::assert_eq!(
            landing_focus,
            Some(FocusedPane {
                id: 99,
                label: Some(FocusedPaneLabel::Title("Cursor …".to_string())),
            })
        );
    }
}
