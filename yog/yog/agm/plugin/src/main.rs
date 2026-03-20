use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use agm_core::Agent;
use agm_core::AgentEvent;
use agm_core::AgentEventKind;
use agm_core::GitStat;
use agm_core::TabStateEntry;
use ui::TabRow;
use zellij_tile::prelude::*;

mod ui;

const REFRESH_INTERVAL_SECS: f64 = 10.0;
const CONTEXT_KEY_GIT_STAT: &str = "git-stat";

#[derive(Default)]
pub struct PaneData {
    pub cwd: Option<PathBuf>,
    pub command: Option<String>,
    pub is_busy: bool,
    pub agent_kind: Option<Agent>,
}

impl PaneData {
    fn ensure_cwd(&mut self, pane_id: u32) -> bool {
        if self.cwd.is_some() {
            return false;
        }
        if let Ok(cwd) = get_pane_cwd(PaneId::Terminal(pane_id)) {
            self.cwd = Some(cwd);
            return true;
        }
        false
    }

    fn apply_title(&mut self, title: &str) -> bool {
        if self.agent_kind.is_some() {
            return false;
        }
        let cmd = parse_pane_title(title);
        if cmd == self.command {
            return false;
        }
        self.command = cmd;
        true
    }

    fn apply_agent_event(&mut self, event: &AgentEvent) -> bool {
        if let Some(current) = self.agent_kind {
            if event.agent.priority() < current.priority() {
                return false;
            }
        }
        match event.kind {
            AgentEventKind::Start | AgentEventKind::Busy => {
                let changed_kind = self.agent_kind != Some(event.agent);
                self.agent_kind = Some(event.agent);
                self.command = None;
                let was_busy = self.is_busy;
                self.is_busy = event.kind == AgentEventKind::Busy;
                self.ensure_cwd(event.pane_id);
                changed_kind || self.is_busy != was_busy
            }
            AgentEventKind::Idle => {
                if self.agent_kind != Some(event.agent) {
                    self.agent_kind = Some(event.agent);
                    self.command = None;
                }
                let was_busy = self.is_busy;
                self.is_busy = false;
                was_busy
            }
            AgentEventKind::Exit => {
                let had_agent = self.agent_kind.is_some();
                self.agent_kind = None;
                self.is_busy = false;
                had_agent
            }
        }
    }
}

#[derive(Default, Eq, PartialEq)]
struct TabPanes {
    focused: Option<u32>,
    all: Vec<u32>,
}

#[derive(Default)]
struct State {
    plugin_id: u32,
    my_tab_id: Option<usize>,
    session_name: String,

    tabs: Vec<TabInfo>,
    pos_tab_id: HashMap<usize, usize>,

    panes_data: HashMap<u32, PaneData>,
    tab_panes: HashMap<usize, TabPanes>,
    pane_to_tab: HashMap<u32, usize>,

    tab_git_stats: HashMap<usize, GitStat>,

    last_manifest: Option<PaneManifest>,
    home_dir: Option<PathBuf>,
    got_permissions: bool,
    did_initial_cleanup: bool,
    frame_dirty: bool,
    last_cols: usize,
    last_frame: Option<Vec<TabRow>>,
    render_buf: String,
}

register_plugin!(State);

impl State {
    // ------------------------------------------------------------------
    // Identity discovery
    // ------------------------------------------------------------------

    fn discover_my_tab(&mut self, manifest: &PaneManifest) {
        if self.my_tab_id.is_some() {
            return;
        }
        for (tab_pos, panes) in &manifest.panes {
            for pane in panes {
                if pane.is_plugin
                    && pane.id == self.plugin_id
                    && let Some(&tab_id) = self.pos_tab_id.get(tab_pos)
                {
                    self.my_tab_id = Some(tab_id);
                    self.detect_own_agents();
                    self.refresh_other_tabs();
                    self.fire_own_git_stat();
                    self.persist_own_state();
                    return;
                }
            }
        }
    }

    fn detect_own_agents(&mut self) {
        let Some(my) = self.my_tab_id else { return };
        let Some(tp) = self.tab_panes.get(&my) else { return };
        let pids: Vec<u32> = tp.all.clone();
        for pid in pids {
            let pd = self.panes_data.entry(pid).or_default();
            if pd.agent_kind.is_none() {
                if let Some(agent) = detect_agent_from_running_command(pid) {
                    pd.agent_kind = Some(agent);
                    pd.command = None;
                    pd.is_busy = false;
                    pd.ensure_cwd(pid);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Pane→tab mapping
    // ------------------------------------------------------------------

    fn rebuild_pane_to_tab(&mut self) {
        self.pane_to_tab.clear();
        for (&tab_id, tp) in &self.tab_panes {
            for &pid in &tp.all {
                self.pane_to_tab.insert(pid, tab_id);
            }
        }
    }

    fn tab_of_pane(&self, pane_id: u32) -> Option<usize> {
        self.pane_to_tab.get(&pane_id).copied()
    }

    fn is_own_pane(&self, pane_id: u32) -> bool {
        self.my_tab_id.is_some_and(|my| self.tab_of_pane(pane_id) == Some(my))
    }

    // ------------------------------------------------------------------
    // Own-tab helpers
    // ------------------------------------------------------------------

    fn own_focused_cwd(&self) -> Option<PathBuf> {
        let my = self.my_tab_id?;
        let focused = self.tab_panes.get(&my)?.focused?;
        self.panes_data.get(&focused)?.cwd.clone()
    }

    fn is_own_agent_busy(&self) -> bool {
        let Some(my) = self.my_tab_id else { return false };
        let Some(tp) = self.tab_panes.get(&my) else {
            return false;
        };
        tp.all.iter().any(|pid| {
            self.panes_data
                .get(pid)
                .is_some_and(|pd| pd.agent_kind.is_some() && pd.is_busy)
        })
    }

    // ------------------------------------------------------------------
    // Git stat (own tab only)
    // ------------------------------------------------------------------

    fn fire_own_git_stat(&self) {
        let Some(cwd) = self.own_focused_cwd() else { return };
        let cwd_str = cwd.display().to_string();
        let args: Vec<&str> = vec!["agm", "git-stat", &cwd_str];
        let mut ctx = BTreeMap::new();
        ctx.insert(CONTEXT_KEY_GIT_STAT.into(), String::new());
        run_command_with_env_variables_and_cwd(&args, BTreeMap::new(), cwd, ctx);
    }

    fn handle_git_stat_result(&mut self, exit_code: Option<i32>, stdout: &[u8]) -> bool {
        if exit_code != Some(0) {
            return false;
        }
        let Some(my_tab) = self.my_tab_id else {
            return false;
        };
        let output = String::from_utf8_lossy(stdout);
        let mut changed = false;
        for line in output.lines() {
            match GitStat::parse_line(line) {
                Ok((_path, stat)) => {
                    let entry = self.tab_git_stats.entry(my_tab).or_default();
                    if *entry != stat {
                        *entry = stat;
                        changed = true;
                    }
                }
                Err(e) => eprintln!("agm: {e}"),
            }
        }
        if changed {
            self.frame_dirty = true;
            self.persist_own_state();
        }
        changed
    }

    // ------------------------------------------------------------------
    // State persistence (own tab → file, immediate on change)
    // ------------------------------------------------------------------

    fn persist_own_state(&self) {
        let Some(my_tab) = self.my_tab_id else { return };

        let cwd = self.own_focused_cwd();
        let (agent, agent_busy) = self.own_agent_info();
        let git_stat = self.tab_git_stats.get(&my_tab).copied().unwrap_or_default();
        let command = focused_pane_data(my_tab, &self.tab_panes, &self.panes_data).and_then(|pd| pd.command.clone());

        let entry = TabStateEntry {
            tab_id: my_tab,
            cwd,
            agent,
            agent_busy,
            git_stat,
            command,
        };
        if let Err(e) = agm_core::write_state_file(&self.session_name, my_tab, &entry.to_file_content()) {
            eprintln!("agm: persist: {e}");
        }
    }

    fn own_agent_info(&self) -> (Option<Agent>, bool) {
        let Some(my) = self.my_tab_id else { return (None, false) };
        let Some(tp) = self.tab_panes.get(&my) else {
            return (None, false);
        };
        for &pid in &tp.all {
            if let Some(pd) = self.panes_data.get(&pid)
                && let Some(agent) = pd.agent_kind
            {
                return (Some(agent), pd.is_busy);
            }
        }
        (None, false)
    }

    // ------------------------------------------------------------------
    // State bootstrap & refresh (read other tabs' files, direct I/O)
    // ------------------------------------------------------------------

    fn refresh_other_tabs(&mut self) -> bool {
        let entries = agm_core::read_all_state_files(&self.session_name);
        let mut changed = false;
        for entry in entries {
            if Some(entry.tab_id) == self.my_tab_id {
                continue;
            }
            let current = self.tab_git_stats.entry(entry.tab_id).or_default();
            if *current != entry.git_stat {
                *current = entry.git_stat;
                changed = true;
            }
            if let Some(agent) = entry.agent {
                if let Some(tp) = self.tab_panes.get(&entry.tab_id) {
                    let target = tp
                        .all
                        .iter()
                        .find(|pid| {
                            self.panes_data
                                .get(pid)
                                .is_some_and(|pd| pd.agent_kind.is_some())
                        })
                        .or(tp.all.first())
                        .copied();
                    if let Some(pid) = target {
                        let pd = self.panes_data.entry(pid).or_default();
                        if pd.agent_kind != Some(agent) {
                            pd.agent_kind = Some(agent);
                            pd.command = None;
                            changed = true;
                        }
                        if pd.is_busy != entry.agent_busy {
                            pd.is_busy = entry.agent_busy;
                            changed = true;
                        }
                    }
                }
            }
        }
        if changed {
            self.frame_dirty = true;
        }
        changed
    }

    // ------------------------------------------------------------------
    // Pane manifest processing
    // ------------------------------------------------------------------

    fn process_pane_manifest(&mut self, manifest: &PaneManifest) -> bool {
        let mut changed = false;

        for (tab_pos, panes) in &manifest.panes {
            let Some(&tab_id) = self.pos_tab_id.get(tab_pos) else {
                continue;
            };

            let mut all_ids: Vec<u32> = Vec::new();
            let mut focused_id: Option<u32> = None;

            for pane in panes.iter().filter(|p| !p.is_plugin) {
                all_ids.push(pane.id);

                if pane.is_focused {
                    focused_id = Some(pane.id);
                    changed |= self.panes_data.entry(pane.id).or_default().ensure_cwd(pane.id);
                }

                let pane_data = self.panes_data.entry(pane.id).or_default();
                let title_changed = pane_data.apply_title(&pane.title);
                changed |= title_changed;

                if title_changed
                    && pane_data.agent_kind.is_none()
                    && self.my_tab_id == Some(tab_id)
                    && let Some(agent) = detect_agent_from_running_command(pane.id)
                {
                    pane_data.agent_kind = Some(agent);
                    pane_data.command = None;
                    pane_data.is_busy = false;
                    pane_data.ensure_cwd(pane.id);
                    changed = true;
                }
            }

            let entry = self.tab_panes.entry(tab_id).or_default();
            let new = TabPanes {
                focused: focused_id,
                all: all_ids,
            };
            if *entry != new {
                *entry = new;
                changed = true;
            }
        }

        changed
    }

    fn prune_stale_entries(&mut self) {
        let active_tab_ids: HashSet<usize> = self.tabs.iter().map(|t| t.tab_id).collect();

        let removed: Vec<usize> = self
            .tab_panes
            .keys()
            .filter(|tid| !active_tab_ids.contains(tid))
            .copied()
            .collect();

        for tid in &removed {
            self.tab_panes.remove(tid);
            self.tab_git_stats.remove(tid);
            agm_core::remove_state_file(&self.session_name, *tid);
        }

        let mut active_pane_ids: HashSet<u32> = HashSet::new();
        for tp in self.tab_panes.values() {
            active_pane_ids.extend(&tp.all);
            if let Some(f) = tp.focused {
                active_pane_ids.insert(f);
            }
        }
        self.panes_data.retain(|pid, _| active_pane_ids.contains(pid));
    }

    fn rebuild_pos_tab_id(&mut self) {
        self.pos_tab_id.clear();
        for t in &self.tabs {
            self.pos_tab_id.insert(t.position, t.tab_id);
        }
    }

    fn handle_agent_pipe(&mut self, msg: &PipeMessage) -> bool {
        let event = match parse_pipe_msg(msg) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("agm: {e}");
                return false;
            }
        };
        let is_own = self.is_own_pane(event.pane_id);
        let changed = self
            .panes_data
            .entry(event.pane_id)
            .or_default()
            .apply_agent_event(&event);
        if changed && is_own {
            self.persist_own_state();
        }
        changed
    }

    fn sync_frame(&mut self) -> bool {
        if !self.frame_dirty {
            return false;
        }
        self.frame_dirty = false;
        let new_frame = compute_frame(
            &self.tabs,
            &self.tab_panes,
            &self.panes_data,
            &self.tab_git_stats,
            self.home_dir.as_deref(),
        );
        let changed = self.last_frame.as_ref().is_none_or(|old| *old != new_frame);
        self.last_frame = Some(new_frame);
        changed
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.plugin_id = get_plugin_ids().plugin_id;
        self.session_name = configuration
            .get("session")
            .cloned()
            .or_else(|| std::env::var("ZELLIJ_SESSION_NAME").ok())
            .unwrap_or_else(|| "default".into());
        self.home_dir = std::env::var_os("HOME").map(PathBuf::from);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
            PermissionType::FullHdAccess,
        ]);
        subscribe(&[EventType::PermissionRequestResult]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                self.got_permissions = true;
                subscribe(&[
                    EventType::TabUpdate,
                    EventType::PaneUpdate,
                    EventType::CwdChanged,
                    EventType::Mouse,
                    EventType::RunCommandResult,
                    EventType::Timer,
                ]);
                set_selectable(false);
                set_timeout(REFRESH_INTERVAL_SECS);
                self.frame_dirty = true;
                self.sync_frame()
            }

            Event::TabUpdate(tabs) => {
                let count_shrunk = tabs.len() < self.tabs.len();
                self.tabs = tabs;
                self.rebuild_pos_tab_id();
                if !self.did_initial_cleanup {
                    self.did_initial_cleanup = true;
                    let active: HashSet<usize> = self.tabs.iter().map(|t| t.tab_id).collect();
                    agm_core::clean_stale_state_files(&self.session_name, &active);
                }
                if count_shrunk {
                    self.prune_stale_entries();
                }
                if let Some(manifest) = self.last_manifest.clone() {
                    self.process_pane_manifest(&manifest);
                    self.rebuild_pane_to_tab();
                    self.discover_my_tab(&manifest);
                }
                self.frame_dirty = true;
                self.sync_frame()
            }

            Event::PaneUpdate(manifest) => {
                let data_changed = self.process_pane_manifest(&manifest);
                self.last_manifest = Some(manifest.clone());
                self.rebuild_pane_to_tab();
                self.discover_my_tab(&manifest);

                if data_changed {
                    if self.my_tab_id.is_some() && !self.tab_git_stats.contains_key(&self.my_tab_id.unwrap()) {
                        self.fire_own_git_stat();
                    }
                    self.persist_own_state();
                    self.frame_dirty = true;
                }
                self.sync_frame()
            }

            Event::CwdChanged(PaneId::Terminal(terminal_id), new_cwd, _clients) => {
                let pane_data = self.panes_data.entry(terminal_id).or_default();
                if pane_data.cwd.as_ref() == Some(&new_cwd) {
                    return false;
                }
                pane_data.cwd = Some(new_cwd);

                let is_own = self.is_own_pane(terminal_id);
                if is_own {
                    self.fire_own_git_stat();
                    self.persist_own_state();
                } else if let Some(tab_id) = self.tab_of_pane(terminal_id) {
                    self.tab_git_stats.remove(&tab_id);
                }

                self.frame_dirty = true;
                self.sync_frame()
            }

            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                if !context.contains_key(CONTEXT_KEY_GIT_STAT) {
                    return false;
                }
                let changed = self.handle_git_stat_result(exit_code, &stdout);
                if changed { self.sync_frame() } else { false }
            }

            Event::Timer(_) => {
                if self.my_tab_id.is_some() {
                    if self.is_own_agent_busy() {
                        self.fire_own_git_stat();
                    }
                    self.refresh_other_tabs();
                }
                set_timeout(REFRESH_INTERVAL_SECS);
                self.sync_frame()
            }

            Event::Mouse(Mouse::LeftClick(row, _col)) => {
                if let Ok(row_u) = usize::try_from(row) {
                    let content_w = self.last_cols.saturating_sub(1);
                    let frame = self.last_frame.as_deref().unwrap_or_default();
                    if let Some(tab_idx) = ui::tab_index_at_row(frame, row_u, content_w)
                        && let Some(tab) = self.tabs.get(tab_idx)
                        && let Ok(pos) = u32::try_from(tab.position)
                    {
                        switch_tab_to(pos + 1);
                    }
                }
                false
            }

            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.last_cols = cols;
        self.render_buf.clear();
        let frame = self.last_frame.as_deref().unwrap_or_default();
        ui::render_frame(frame, rows, cols, &mut self.render_buf);
        if !self.render_buf.is_empty() {
            print!("{}", self.render_buf);
        }
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        if pipe_message.name != agm_core::PIPE_NAME {
            return false;
        }
        if !self.handle_agent_pipe(&pipe_message) {
            return false;
        }
        self.frame_dirty = true;
        self.sync_frame()
    }
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

fn parse_pipe_msg(msg: &PipeMessage) -> Result<AgentEvent, agm_core::ParseError> {
    let raw_id = msg
        .args
        .get("pane_id")
        .ok_or_else(|| agm_core::ParseError::new("missing pane_id"))?;
    let raw_agent = msg
        .args
        .get("agent")
        .ok_or_else(|| agm_core::ParseError::new("missing agent"))?;
    let raw_payload = msg.payload.as_deref().unwrap_or("");
    AgentEvent::parse(raw_id, raw_agent, raw_payload)
}

fn focused_pane_data<'a>(
    tab_id: usize,
    tab_panes: &HashMap<usize, TabPanes>,
    panes_data: &'a HashMap<u32, PaneData>,
) -> Option<&'a PaneData> {
    tab_panes.get(&tab_id)?.focused.and_then(|pid| panes_data.get(&pid))
}

fn priority_command_for_tab(
    tab_id: usize,
    tab_panes: &HashMap<usize, TabPanes>,
    panes_data: &HashMap<u32, PaneData>,
) -> Option<(&'static str, bool)> {
    for pid in &tab_panes.get(&tab_id)?.all {
        if let Some(pane_data) = panes_data.get(pid)
            && let Some(kind) = pane_data.agent_kind
        {
            return Some((kind.name(), pane_data.is_busy));
        }
    }
    None
}

fn compute_frame(
    tabs: &[TabInfo],
    tab_panes: &HashMap<usize, TabPanes>,
    panes_data: &HashMap<u32, PaneData>,
    tab_git_stats: &HashMap<usize, GitStat>,
    home: Option<&Path>,
) -> Vec<TabRow> {
    tabs.iter()
        .map(|tab| {
            let focused = focused_pane_data(tab.tab_id, tab_panes, panes_data);
            let priority_cmd = priority_command_for_tab(tab.tab_id, tab_panes, panes_data);
            let git = tab_git_stats.get(&tab.tab_id).copied().unwrap_or_default();
            TabRow::new(tab, focused, priority_cmd, git, home)
        })
        .collect()
}

fn detect_agent_from_running_command(pane_id: u32) -> Option<Agent> {
    let args = get_pane_running_command(PaneId::Terminal(pane_id)).ok()?;
    args.iter()
        .filter_map(|arg| {
            let basename = arg.rsplit('/').next().unwrap_or(arg);
            Agent::detect(basename)
        })
        .max_by_key(|a| a.priority())
}

fn parse_pane_title(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.starts_with('~') || trimmed.starts_with('/') {
        return None;
    }
    let name = trimmed.split_whitespace().next().unwrap_or("");
    let name = name.rsplit('/').next().unwrap_or(name);
    if name.is_empty() || name == "zsh" || name == "bash" || name == "fish" {
        return None;
    }
    Some(name.to_owned())
}
