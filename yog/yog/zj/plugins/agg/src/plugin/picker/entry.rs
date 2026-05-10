use std::path::Path;
use std::path::PathBuf;

use agg::GitStat;
use agg::TabIndicator;
use ytil_agents::agent::Agent;
use ytil_agents::agent::AgentEventKind;
use ytil_agents::agent::AgentEventPayload;
use zellij_tile::prelude::TabInfo;

use crate::plugin::picker::state::PaneObservation;
use crate::plugin::picker::state::SessionEntry;
use crate::plugin::picker::ui::PickerRow;

#[cfg_attr(test, derive(Debug))]
#[derive(Clone, Eq, PartialEq)]
pub struct PaneEntry {
    pub tab_position: usize,
    tab_number: usize,
    tab_id: Option<usize>,
    pub pane_id: u32,
    pub cwd: Option<PathBuf>,
    pub command_args: Vec<String>,
    label: Option<String>,
    marker: TabIndicator,
    agent: Option<Agent>,
    session_id: Option<String>,
    tab_active: bool,
    is_focused: bool,
    git: GitStat,
    pub session_summary: Option<String>,
    pub session_display: Option<String>,
    pub session_search: Option<String>,
}

impl PaneEntry {
    pub fn from_observation(
        pane: &PaneObservation,
        cached_cwd: Option<PathBuf>,
        cached_command: Option<Vec<String>>,
        tabs: &[TabInfo],
    ) -> Self {
        let command_args = cached_command
            .or_else(|| pane.terminal_command_args.clone())
            .unwrap_or_default();
        let mut entry = Self::new(
            pane.tab_position,
            pane.pane_id,
            cached_cwd,
            command_args,
            pane.title_label.clone(),
        );
        entry.is_focused = pane.is_focused;
        entry.apply_tab_metadata(tabs);
        entry
    }

    pub fn new(
        tab_position: usize,
        pane_id: u32,
        cwd: Option<PathBuf>,
        command_args: Vec<String>,
        title_label: Option<String>,
    ) -> Self {
        let mut entry = Self {
            tab_position,
            tab_number: tab_position.saturating_add(1),
            tab_id: None,
            pane_id,
            cwd,
            command_args,
            label: title_label,
            marker: TabIndicator::NoAgent,
            agent: None,
            session_id: None,
            tab_active: false,
            is_focused: false,
            git: GitStat::default(),
            session_summary: None,
            session_display: None,
            session_search: None,
        };
        entry.apply_command(entry.command_args.clone());
        if entry.agent.is_none() {
            entry.agent = entry.label.as_deref().and_then(Agent::detect);
            if let Some(agent) = entry.agent {
                entry.marker = TabIndicator::Seen;
                entry.label = Some(agent.short_name().to_string());
            }
        }
        entry
    }

    pub fn apply_command(&mut self, command_args: Vec<String>) {
        self.command_args = command_args;
        self.agent = crate::plugin::pane::agent_from_command_args(&self.command_args);
        self.session_id = crate::plugin::picker::entry::resume_session_id_from_command_args(&self.command_args);
        self.marker = if self.agent.is_some() {
            TabIndicator::Seen
        } else {
            TabIndicator::NoAgent
        };
        self.label = self
            .agent
            .map(|agent| agent.short_name().to_string())
            .or_else(|| crate::plugin::pane::label_from_command_args(&self.command_args))
            .or_else(|| self.label.clone());
    }

    pub fn inherit_agent_state(&mut self, previous: &Self) {
        if self.agent.is_some() && self.agent == previous.agent {
            self.marker = previous.marker;
            if self.tab_active && self.is_focused && self.marker == TabIndicator::Unseen {
                self.marker = TabIndicator::Seen;
            }
        }
    }

    pub fn apply_agent_event(&mut self, event: &AgentEventPayload) -> bool {
        if self.pane_id != event.pane_id {
            return false;
        }
        if self
            .agent
            .is_some_and(|agent| event.agent.priority() < agent.priority())
        {
            return false;
        }

        let old_agent = self.agent;
        let old_marker = self.marker;
        let old_label = self.label.clone();
        match event.kind {
            AgentEventKind::Start => {
                self.agent = Some(event.agent);
                self.marker = TabIndicator::Seen;
                self.label = Some(event.agent.short_name().to_string());
            }
            AgentEventKind::Busy => {
                self.agent = Some(event.agent);
                self.marker = TabIndicator::Busy;
                self.label = Some(event.agent.short_name().to_string());
            }
            AgentEventKind::Idle => {
                self.agent = Some(event.agent);
                self.marker = if self.tab_active && self.is_focused {
                    TabIndicator::Seen
                } else {
                    TabIndicator::Unseen
                };
                self.label = Some(event.agent.short_name().to_string());
            }
            AgentEventKind::Exit => {
                if self.agent == Some(event.agent) {
                    self.agent = None;
                    self.marker = TabIndicator::NoAgent;
                    self.label = crate::plugin::pane::label_from_command_args(&self.command_args);
                }
            }
        }
        old_agent != self.agent || old_marker != self.marker || old_label != self.label
    }

    pub fn apply_git_stat(&mut self, stat: GitStat) -> bool {
        let changed = self.git != stat;
        self.git = stat;
        changed
    }

    pub fn attach_session(&mut self, sessions: &[SessionEntry]) {
        self.session_summary = None;
        self.session_display = None;
        self.session_search = None;
        let Some(session) = crate::plugin::picker::entry::matching_session(self, sessions) else {
            return;
        };
        self.session_summary.clone_from(&session.summary);
        self.session_display = Some(session.display.clone());
        self.session_search = Some(session.search.clone());
    }

    pub fn apply_tab_metadata(&mut self, tabs: &[TabInfo]) {
        if let Some(tab) = tabs.iter().find(|tab| tab.position == self.tab_position) {
            self.tab_number = tab.position.saturating_add(1);
            self.tab_id = Some(tab.tab_id);
            self.tab_active = tab.active;
            if self.tab_active && self.is_focused && self.marker == TabIndicator::Unseen {
                self.marker = TabIndicator::Seen;
            }
        }
    }

    pub fn matches_query(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        query.is_empty() || self.search_text().contains(&query)
    }

    pub fn row(&self, selected: bool, home_dir: &Path) -> PickerRow {
        PickerRow {
            selected,
            cwd_label: self
                .cwd
                .as_ref()
                .map_or_else(|| String::from("-"), |cwd| ytil_tui::short_path(cwd, home_dir)),
            summary: self.session_summary.clone().unwrap_or_default(),
            label: self.label.clone().unwrap_or_default(),
            marker: self.marker,
            git: self.git,
        }
    }

    fn search_text(&self) -> String {
        let cwd = self
            .cwd
            .as_ref()
            .map_or_else(String::new, |cwd| cwd.display().to_string());
        let label = self.label.as_deref().unwrap_or_default();
        let command = self.command_args.join(" ");
        let session_display = self.session_display.as_deref().unwrap_or_default();
        let session_search = self.session_search.as_deref().unwrap_or_default();
        format!(
            "{} {} {} {} {} {} {}",
            self.tab_number, self.pane_id, cwd, command, label, session_display, session_search
        )
        .to_ascii_lowercase()
    }
}

pub fn sort_by_tab_order(pane_entries: &mut [PaneEntry], tabs: &[TabInfo]) {
    pane_entries.sort_by_key(|entry| {
        let tab_order = tabs
            .iter()
            .position(|tab| tab.position == entry.tab_position)
            .unwrap_or_else(|| tabs.len().saturating_add(entry.tab_position));
        (tab_order, entry.pane_id)
    });
}

fn matching_session<'a>(entry: &PaneEntry, sessions: &'a [SessionEntry]) -> Option<&'a SessionEntry> {
    let agent = entry.agent?;
    let session_id = entry.session_id.as_deref()?;
    sessions
        .iter()
        .find(|session| session.agent == agent.name() && session.session_id == session_id)
}

fn resume_session_id_from_command_args(args: &[String]) -> Option<String> {
    let command = args
        .first()
        .map(String::as_str)
        .map(crate::plugin::pane::command_name)?;
    match command {
        "codex" => crate::plugin::picker::entry::codex_resume_id(args),
        "claude" | "cursor-agent" => crate::plugin::picker::entry::resume_flag_id(args),
        _ => None,
    }
}

fn codex_resume_id(args: &[String]) -> Option<String> {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "resume" {
            return iter.next().cloned();
        }
    }
    None
}

fn resume_flag_id(args: &[String]) -> Option<String> {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--resume" {
            return iter.next().cloned();
        }
    }
    None
}
