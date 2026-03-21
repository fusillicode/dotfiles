use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use agm_core::Agent;
use agm_core::AgentEvent;
use agm_core::AgentEventKind;
use agm_core::Cmd;
use agm_core::GitStat;
use agm_core::TabStateEntry;
use ui::TabRow;
use zellij_tile::prelude::*;

mod ui;

const REFRESH_INTERVAL_SECS: f64 = 5.0;
const CONTEXT_KEY_GIT_STAT: &str = "git-stat";

#[derive(Default)]
pub struct PaneData {
    pub cwd: Option<PathBuf>,
    pub cmd: Cmd,
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
        if self.cmd.is_agent() {
            return false;
        }
        let cmd = parse_pane_title(title);
        let new_cmd = cmd.map_or_else(|| Cmd::None, Cmd::Running);
        if self.cmd == new_cmd {
            return false;
        }
        self.cmd = new_cmd;
        true
    }

    fn apply_agent_event(&mut self, event: &AgentEvent) -> bool {
        // Determine current agent (if any) from command enum
        let current_agent = self.cmd.agent_name().and_then(|name| Agent::from_name(name).ok());
        if let Some(current) = current_agent
            && event.agent.priority() < current.priority()
        {
            return false;
        }
        let new_cmd = Cmd::from(event);
        let changed = self.cmd != new_cmd;
        self.cmd = new_cmd;
        if changed
            && matches!(
                event.kind,
                AgentEventKind::Start | AgentEventKind::Busy | AgentEventKind::Idle
            )
        {
            self.ensure_cwd(event.pane_id);
        }
        changed
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
    home_dir: PathBuf,
    got_permissions: bool,
    frame_dirty: bool,
    last_cols: usize,
    last_frame: Option<Vec<TabRow>>,
    render_buf: String,
}

register_plugin!(State);

impl State {
    fn find_my_tab_id(&self, manifest: &PaneManifest) -> Option<usize> {
        if self.my_tab_id.is_some() {
            return None;
        }
        for (tab_pos, panes) in &manifest.panes {
            for pane in panes {
                if pane.is_plugin
                    && pane.id == self.plugin_id
                    && let Some(&tab_id) = self.pos_tab_id.get(tab_pos)
                {
                    return Some(tab_id);
                }
            }
        }
        None
    }

    fn refresh_state(&mut self, tab_id: usize) {
        self.my_tab_id = Some(tab_id);
        self.detect_own_agents();
        self.refresh_other_tabs();
        self.fire_own_git_stat();
        self.persist_own_state();
    }

    fn detect_own_agents(&mut self) {
        let Some(my) = self.my_tab_id else { return };
        let Some(tp) = self.tab_panes.get(&my) else { return };
        let pids: Vec<u32> = tp.all.clone();
        for pid in pids {
            let pd = self.panes_data.entry(pid).or_default();
            if !pd.cmd.is_agent()
                && let Some(agent) = detect_agent_from_running_command(pid)
            {
                pd.cmd = Cmd::IdleAgent(agent);
                pd.ensure_cwd(pid);
            }
        }
    }

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

    fn own_focused_cwd(&self) -> Option<PathBuf> {
        let my = self.my_tab_id?;
        let focused = self.tab_panes.get(&my)?.focused?;
        self.panes_data.get(&focused)?.cwd.clone()
    }

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

    fn persist_own_state(&self) {
        let Some(tab_id) = self.my_tab_id else { return };

        let agent_cmd = self.own_agent_cmd();
        let cmd = if agent_cmd.is_agent() {
            agent_cmd
        } else {
            focused_pane_data(tab_id, &self.tab_panes, &self.panes_data)
                .map(|pd| pd.cmd.clone())
                .unwrap_or(Cmd::None)
        };

        let entry = TabStateEntry {
            tab_id,
            cwd: self.own_focused_cwd(),
            cmd,
            git_stat: self.tab_git_stats.get(&tab_id).copied().unwrap_or_default(),
        };

        if let Err(e) = agm_core::write_state_file(&self.session_name, tab_id, &entry.to_file_content()) {
            eprintln!("agm: persist: {e}");
        }
    }

    fn own_agent_cmd(&self) -> Cmd {
        let Some(tab_id) = self.my_tab_id else { return Cmd::None };
        let Some(tab_panes) = self.tab_panes.get(&tab_id) else {
            return Cmd::None;
        };
        for &pid in &tab_panes.all {
            if let Some(pane_data) = self.panes_data.get(&pid)
                && pane_data.cmd.is_agent()
            {
                return pane_data.cmd.clone();
            }
        }
        Cmd::None
    }

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
            // Skip if no agent in this entry
            if !entry.cmd.is_agent() {
                continue;
            }
            let Some(tab_pane) = self.tab_panes.get(&entry.tab_id) else {
                continue;
            };
            let target = tab_pane
                .all
                .iter()
                .find(|pid| self.panes_data.get(pid).is_some_and(|pd| pd.cmd.is_agent()))
                .or(tab_pane.all.first())
                .copied();
            let Some(pid) = target else { continue };
            let pd = self.panes_data.entry(pid).or_default();
            if pd.cmd != entry.cmd {
                pd.cmd = entry.cmd.clone();
                changed = true;
            }
        }
        if changed {
            self.frame_dirty = true;
        }
        changed
    }

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
                    && !pane_data.cmd.is_agent()
                    && self.my_tab_id == Some(tab_id)
                    && let Some(agent) = detect_agent_from_running_command(pane.id)
                {
                    pane_data.cmd = Cmd::IdleAgent(agent);
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

        let mut removed_pane_ids = HashSet::new();
        let removed: Vec<usize> = self
            .tab_panes
            .keys()
            .filter(|tid| !active_tab_ids.contains(tid))
            .copied()
            .collect();

        for tid in &removed {
            if let Some(tp) = self.tab_panes.remove(tid) {
                self.tab_git_stats.remove(tid);
                agm_core::remove_state_file(&self.session_name, *tid);
                removed_pane_ids.extend(tp.all);
                if let Some(focused) = tp.focused {
                    removed_pane_ids.insert(focused);
                }
            }
        }

        self.panes_data.retain(|pid, _| !removed_pane_ids.contains(pid));
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
        let new_frame = compute_frame(self);
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
        self.home_dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .expect("error getting HOME env var");
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
                if count_shrunk {
                    self.prune_stale_entries();
                }
                if let Some(manifest) = self.last_manifest.clone() {
                    self.process_pane_manifest(&manifest);
                    self.rebuild_pane_to_tab();
                    if let Some(tab_id) = self.find_my_tab_id(&manifest) {
                        self.refresh_state(tab_id);
                    }
                }
                self.frame_dirty = true;
                self.sync_frame()
            }

            Event::PaneUpdate(manifest) => {
                let data_changed = self.process_pane_manifest(&manifest);
                self.last_manifest = Some(manifest.clone());
                self.rebuild_pane_to_tab();
                if let Some(tab_id) = self.find_my_tab_id(&manifest) {
                    self.refresh_state(tab_id);
                }

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
                    self.fire_own_git_stat();
                    self.refresh_other_tabs();
                }
                set_timeout(REFRESH_INTERVAL_SECS);
                self.sync_frame()
            }

            Event::Mouse(Mouse::LeftClick(row, _col)) => {
                let Ok(row_u) = usize::try_from(row) else { return false };
                let content_w = self.last_cols.saturating_sub(1);
                let frame = self.last_frame.as_deref().unwrap_or_default();
                if let Some(tab_idx) = ui::tab_index_at_row(frame, row_u, content_w)
                    && let Some(tab) = self.tabs.get(tab_idx)
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
) -> Option<Cmd> {
    for pid in &tab_panes.get(&tab_id)?.all {
        if let Some(pane_data) = panes_data.get(pid)
            && pane_data.cmd.agent_name().is_some()
        {
            return Some(pane_data.cmd.clone());
        }
    }
    None
}

fn compute_frame(state: &State) -> Vec<TabRow> {
    state
        .tabs
        .iter()
        .map(|tab| {
            let focused = focused_pane_data(tab.tab_id, &state.tab_panes, &state.panes_data);
            let priority_cmd = priority_command_for_tab(tab.tab_id, &state.tab_panes, &state.panes_data);
            let git = state.tab_git_stats.get(&tab.tab_id).copied().unwrap_or_default();
            TabRow::new(tab, focused, priority_cmd, git, &state.home_dir)
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
