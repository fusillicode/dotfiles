use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use agm_core::Agent;
use agm_core::AgentEvent;
use agm_core::AgentEventKind;
use agm_core::GitStat;
use agm_core::ParseError;
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

    fn apply_agent_event(&mut self, pane_id: u32, agent: Agent, kind: AgentEventKind) -> bool {
        match kind {
            AgentEventKind::Start | AgentEventKind::Busy => {
                let changed_kind = self.agent_kind != Some(agent);
                self.agent_kind = Some(agent);
                self.command = None;
                let was_busy = self.is_busy;
                self.is_busy = kind == AgentEventKind::Busy;
                self.ensure_cwd(pane_id);
                changed_kind || self.is_busy != was_busy
            }
            AgentEventKind::Idle => {
                if self.agent_kind != Some(agent) {
                    self.agent_kind = Some(agent);
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
    tabs: Vec<TabInfo>,
    panes_data: HashMap<u32, PaneData>,
    tab_panes: HashMap<usize, TabPanes>,
    git_stats: HashMap<PathBuf, GitStat>,
    pos_tab_id: HashMap<usize, usize>,
    last_manifest: Option<PaneManifest>,
    home_dir: Option<PathBuf>,
    got_permissions: bool,
    frame_dirty: bool,
    last_cols: usize,
    last_frame: Option<Vec<TabRow>>,
    render_buf: String,
}

register_plugin!(State);

impl State {
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
        self.tab_panes.retain(|tid, _| active_tab_ids.contains(tid));

        let mut active_pane_ids: HashSet<u32> = HashSet::new();
        for tp in self.tab_panes.values() {
            active_pane_ids.extend(&tp.all);
            if let Some(f) = tp.focused {
                active_pane_ids.insert(f);
            }
        }
        self.panes_data.retain(|pid, _| active_pane_ids.contains(pid));

        let live = visible_cwds(&self.tabs, &self.tab_panes, &self.panes_data);
        self.git_stats.retain(|cwd, _| live.contains(cwd));
    }

    fn rebuild_pos_tab_id(&mut self) {
        self.pos_tab_id.clear();
        for t in &self.tabs {
            self.pos_tab_id.insert(t.position, t.tab_id);
        }
    }

    fn fire_git_diffs(&self, only_missing: bool) {
        let cwds: Vec<String> = visible_cwds(&self.tabs, &self.tab_panes, &self.panes_data)
            .into_iter()
            .filter(|cwd| !only_missing || !self.git_stats.contains_key(cwd))
            .map(|p| p.display().to_string())
            .collect();
        if cwds.is_empty() {
            return;
        }
        let mut args: Vec<&str> = vec!["agm", "git-stat"];
        args.extend(cwds.iter().map(String::as_str));
        let mut ctx = BTreeMap::new();
        ctx.insert(CONTEXT_KEY_GIT_STAT.into(), String::new());
        run_command_with_env_variables_and_cwd(
            &args,
            BTreeMap::new(),
            PathBuf::from(cwds.first().map_or(".", String::as_str)),
            ctx,
        );
    }

    fn handle_agent_pipe(&mut self, msg: &PipeMessage) -> bool {
        let event = match parse_pipe_msg(msg) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("agm: {e}");
                return false;
            }
        };
        self.panes_data
            .entry(event.pane_id)
            .or_default()
            .apply_agent_event(event.pane_id, event.agent, event.kind)
    }

    fn handle_run_result(&mut self, exit_code: Option<i32>, stdout: &[u8], context: &BTreeMap<String, String>) -> bool {
        if !context.contains_key(CONTEXT_KEY_GIT_STAT) || exit_code != Some(0) {
            return false;
        }
        let output = String::from_utf8_lossy(stdout);
        let mut changed = false;
        for line in output.lines() {
            match GitStat::parse_line(line) {
                Ok((path, stat)) => {
                    let entry = self.git_stats.entry(path).or_default();
                    if *entry != stat {
                        *entry = stat;
                        self.frame_dirty = true;
                        changed = true;
                    }
                }
                Err(e) => eprintln!("agm: {e}"),
            }
        }
        if changed { self.sync_frame() } else { false }
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
            &self.git_stats,
            self.home_dir.as_deref(),
        );
        let changed = self.last_frame.as_ref().is_none_or(|old| *old != new_frame);
        self.last_frame = Some(new_frame);
        changed
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        self.home_dir = std::env::var_os("HOME").map(PathBuf::from);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
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
                }
                self.fire_git_diffs(true);
                self.frame_dirty = true;
                self.sync_frame()
            }

            Event::PaneUpdate(manifest) => {
                let data_changed = self.process_pane_manifest(&manifest);
                self.last_manifest = Some(manifest);
                if data_changed {
                    self.fire_git_diffs(true);
                    self.frame_dirty = true;
                }
                self.sync_frame()
            }

            Event::CwdChanged(PaneId::Terminal(terminal_id), new_cwd, _clients) => {
                let pane_data = self.panes_data.entry(terminal_id).or_default();
                if pane_data.cwd.as_ref() != Some(&new_cwd) {
                    pane_data.cwd = Some(new_cwd);
                    self.fire_git_diffs(true);
                    self.frame_dirty = true;
                    return self.sync_frame();
                }
                false
            }

            Event::RunCommandResult(exit_code, stdout, _stderr, context) => {
                self.handle_run_result(exit_code, &stdout, &context)
            }

            Event::Timer(_) => {
                if any_agent_busy(&self.panes_data) {
                    self.fire_git_diffs(false);
                }
                set_timeout(REFRESH_INTERVAL_SECS);
                false
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
        if pipe_message.name == agm_core::PIPE_NAME {
            let changed = self.handle_agent_pipe(&pipe_message);
            if changed {
                self.frame_dirty = true;
                self.sync_frame()
            } else {
                false
            }
        } else {
            false
        }
    }
}

fn parse_pipe_msg(msg: &PipeMessage) -> Result<AgentEvent, ParseError> {
    let raw_id = msg
        .args
        .get("pane_id")
        .ok_or_else(|| ParseError::new("missing pane_id"))?;
    let raw_agent = msg.args.get("agent").ok_or_else(|| ParseError::new("missing agent"))?;
    let raw_payload = msg.payload.as_deref().unwrap_or("");
    AgentEvent::parse(raw_id, raw_agent, raw_payload)
}

fn visible_cwds(
    tabs: &[TabInfo],
    tab_panes: &HashMap<usize, TabPanes>,
    panes_data: &HashMap<u32, PaneData>,
) -> HashSet<PathBuf> {
    let mut seen = HashSet::new();
    for tab in tabs {
        if let Some(pane_data) = focused_pane_data(tab.tab_id, tab_panes, panes_data)
            && let Some(cwd) = &pane_data.cwd
        {
            seen.insert(cwd.clone());
        }
    }
    seen
}

fn any_agent_busy(panes_data: &HashMap<u32, PaneData>) -> bool {
    panes_data.values().any(|e| e.agent_kind.is_some() && e.is_busy)
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
    git_stats: &HashMap<PathBuf, GitStat>,
    home: Option<&Path>,
) -> Vec<TabRow> {
    tabs.iter()
        .map(|tab| {
            let focused = focused_pane_data(tab.tab_id, tab_panes, panes_data);
            let priority_cmd = priority_command_for_tab(tab.tab_id, tab_panes, panes_data);
            TabRow::new(tab, focused, priority_cmd, git_stats, home)
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
