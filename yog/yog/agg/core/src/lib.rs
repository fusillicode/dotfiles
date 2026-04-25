use std::path::Path;
use std::path::PathBuf;

use crate::agent::Agent;
use crate::agent::AgentEventKind;
use crate::agent::AgentEventPayload;
use crate::git_stat::GitStat;

pub mod agent;
pub mod git_stat;

pub const EMPTY_FIELD: &str = "--";

#[derive(Debug, PartialEq)]
pub enum ParseError {
    Missing(&'static str),
    Invalid { field: &'static str, value: String },
}

impl ParseError {
    pub fn invalid(field: &'static str, value: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            value: value.into(),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Missing(field) => write!(f, "missing {field}"),
            ParseError::Invalid { field, value } => write!(f, "invalid {field}: {value}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Render a compact path label using `~/...` when under `home`, abbreviating all
/// parent directories to a single character and keeping the last segment intact.
pub fn short_path(path: &Path, home: &Path) -> String {
    if home != Path::new("/") {
        if path == home {
            return "~".into();
        }
        if let Ok(rel) = path.strip_prefix(home) {
            let names = path_dir_names(rel);
            return if names.is_empty() {
                "~".into()
            } else {
                format!("~/{}", abbrev_path_dirs(&names))
            };
        }
    }

    let names = path_dir_names(path);
    if names.is_empty() {
        "/".into()
    } else {
        format!("/{}", abbrev_path_dirs(&names))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Cmd {
    #[default]
    None,
    Running(String),
    Agent {
        agent: Agent,
        state: AgentState,
    },
}

impl Cmd {
    pub const fn agent(agent: Agent, state: AgentState) -> Self {
        Self::Agent { agent, state }
    }

    pub const fn waiting(agent: Agent, seen: bool) -> Self {
        if seen {
            Self::agent(agent, AgentState::Acknowledged)
        } else {
            Self::agent(agent, AgentState::NeedsAttention)
        }
    }

    pub const fn tracked_agent(&self) -> Option<Agent> {
        match self {
            Self::Agent { agent, .. } => Some(*agent),
            Self::None | Self::Running(_) => None,
        }
    }

    pub const fn agent_state(&self) -> Option<AgentState> {
        match self {
            Self::Agent { state, .. } => Some(*state),
            Self::None | Self::Running(_) => None,
        }
    }

    pub fn agent_name(&self) -> Option<&'static str> {
        self.tracked_agent().map(Agent::name)
    }

    pub fn running_cmd(&self) -> Option<&str> {
        match self {
            Self::Running(s) => Some(s),
            Self::None | Self::Agent { .. } => None,
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Agent {
                state: AgentState::Busy,
                ..
            }
        )
    }

    pub fn needs_attention(&self) -> bool {
        matches!(
            self,
            Self::Agent {
                state: AgentState::NeedsAttention,
                ..
            }
        )
    }

    pub fn acknowledge(&mut self) -> bool {
        let Self::Agent { state, .. } = self else {
            return false;
        };
        if *state != AgentState::NeedsAttention {
            return false;
        }
        *state = AgentState::Acknowledged;
        true
    }

    pub fn from_parts(agent: Option<Agent>, agent_state: Option<AgentState>, command: Option<String>) -> Self {
        let Some(agent) = agent else {
            return command.map_or(Self::None, Self::Running);
        };
        Self::agent(agent, agent_state.unwrap_or(AgentState::Acknowledged))
    }

    pub fn into_parts(self) -> (Option<Agent>, Option<AgentState>, Option<String>) {
        match self {
            Self::None => (None, None, None),
            Self::Running(cmd) => (None, None, Some(cmd)),
            Self::Agent { agent, state } => (Some(agent), Some(state), None),
        }
    }
}

impl From<&AgentEventPayload> for Cmd {
    fn from(value: &AgentEventPayload) -> Self {
        match value.kind {
            AgentEventKind::Start => Self::agent(value.agent, AgentState::Acknowledged),
            AgentEventKind::Busy => Self::agent(value.agent, AgentState::Busy),
            AgentEventKind::Idle => Self::agent(value.agent, AgentState::Acknowledged),
            AgentEventKind::Exit => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TabIndicator {
    #[default]
    None,
    Empty,
    Green,
    Red,
}

impl TabIndicator {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Empty => "empty",
            Self::Green => "green",
            Self::Red => "red",
        }
    }

    pub fn parse(s: &str) -> Result<Self, ParseError> {
        match s {
            "none" => Ok(Self::None),
            "empty" => Ok(Self::Empty),
            "green" => Ok(Self::Green),
            "red" => Ok(Self::Red),
            _ => Err(ParseError::invalid("indicator", format!("{s:?}"))),
        }
    }

    pub const fn from_cmd(cmd: &Cmd) -> Self {
        match cmd {
            Cmd::Agent {
                state: AgentState::NeedsAttention,
                ..
            } => Self::Red,
            Cmd::Agent {
                state: AgentState::Busy,
                ..
            } => Self::Green,
            Cmd::Agent { .. } => Self::Empty,
            Cmd::None | Cmd::Running(_) => Self::None,
        }
    }

    pub const fn normalize_for_cmd(self, cmd: &Cmd) -> Self {
        match (self, cmd) {
            (Self::None, Cmd::Agent { .. }) => Self::from_cmd(cmd),
            (Self::Empty, Cmd::None | Cmd::Running(_)) => Self::None,
            _ => self,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentState {
    Busy,
    NeedsAttention,
    Acknowledged,
}

impl AgentState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::NeedsAttention => "needs_attention",
            Self::Acknowledged => "acknowledged",
        }
    }

    pub fn parse(s: &str) -> Result<Self, ParseError> {
        match s {
            "busy" => Ok(Self::Busy),
            "needs_attention" | "waiting_unseen" => Ok(Self::NeedsAttention),
            "acknowledged" | "waiting_seen" => Ok(Self::Acknowledged),
            _ => Err(ParseError::invalid("agent_state", format!("{s:?}"))),
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct TabStateEntry {
    pub tab_id: usize,
    pub cwd: Option<PathBuf>,
    pub cmd: Cmd,
    pub indicator: TabIndicator,
    pub git_stat: GitStat,
}

impl std::fmt::Display for TabStateEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cwd_s = self.cwd.as_ref().map(|p| p.display().to_string());
        let cmd_s = self.cmd.running_cmd();
        let agent_state = self.cmd.agent_state().map(AgentState::as_str);

        write!(
            f,
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            encode_opt(cwd_s.as_deref()),
            encode_opt(self.cmd.agent_name()),
            encode_opt(agent_state),
            self.indicator.as_str(),
            self.git_stat.insertions,
            self.git_stat.deletions,
            self.git_stat.new_files,
            u8::from(self.git_stat.is_worktree),
            encode_opt(cmd_s),
        )
    }
}

impl std::convert::TryFrom<(usize, &str)> for TabStateEntry {
    type Error = ParseError;

    fn try_from((tab_id, content): (usize, &str)) -> Result<Self, Self::Error> {
        let mut l = content.lines();
        let mut next = |name| l.next().ok_or(ParseError::Missing(name));

        let cwd = decode_opt_path(next("cwd")?);
        let agent_raw = next("agent")?;
        let agent = if agent_raw == EMPTY_FIELD {
            None
        } else {
            Some(Agent::from_name(agent_raw)?)
        };
        let agent_state = match next("agent_state")? {
            EMPTY_FIELD => None,
            "0" => Some(AgentState::Acknowledged),
            "1" => Some(AgentState::Busy),
            value => Some(AgentState::parse(value)?),
        };
        let indicator_or_ins = next("indicator")?;
        let (indicator, insertions, has_explicit_indicator) = match TabIndicator::parse(indicator_or_ins) {
            Ok(indicator) => (indicator, parse_usize(next("ins")?, "ins")?, true),
            Err(_) => (TabIndicator::None, parse_usize(indicator_or_ins, "ins")?, false),
        };
        let deletions = parse_usize(next("del")?, "del")?;
        let new_files = parse_usize(next("new")?, "new")?;
        let is_worktree = parse_bool(next("wt")?, "wt")?;
        let command = decode_opt(next("cmd")?);
        let cmd = Cmd::from_parts(agent, agent_state, command);
        let indicator = if has_explicit_indicator {
            indicator.normalize_for_cmd(&cmd)
        } else {
            TabIndicator::from_cmd(&cmd)
        };

        Ok(Self {
            tab_id,
            cwd,
            cmd,
            indicator,
            git_stat: GitStat {
                insertions,
                deletions,
                new_files,
                is_worktree,
            },
        })
    }
}

fn path_dir_names(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir
            | std::path::Component::ParentDir => None,
        })
        .collect()
}

fn abbrev_path_dirs(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names.first().cloned().unwrap_or_default(),
        total => {
            let mut out = String::new();
            for (idx, name) in names.iter().enumerate() {
                if idx > 0 {
                    out.push('/');
                }
                let is_last = idx == total.saturating_sub(1);
                if is_last {
                    out.push_str(name);
                } else {
                    out.push(name.chars().next().unwrap_or('·'));
                }
            }
            out
        }
    }
}

fn encode_opt(val: Option<&str>) -> &str {
    val.unwrap_or(EMPTY_FIELD)
}

fn decode_opt(val: &str) -> Option<String> {
    if val == EMPTY_FIELD { None } else { Some(val.to_owned()) }
}

fn decode_opt_path(val: &str) -> Option<PathBuf> {
    if val == EMPTY_FIELD {
        None
    } else {
        Some(PathBuf::from(val))
    }
}

fn parse_bool(s: &str, name: &'static str) -> Result<bool, ParseError> {
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(ParseError::Invalid {
            field: name,
            value: format!("{s:?}"),
        }),
    }
}

fn parse_usize(s: &str, name: &'static str) -> Result<usize, ParseError> {
    s.parse().map_err(|_| ParseError::Invalid {
        field: name,
        value: format!("{s:?}"),
    })
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;
    use std::path::Path;

    use super::*;

    #[test]
    fn test_tab_state_entry_serialization_roundtrip_works_as_expected() {
        let entry = TabStateEntry {
            tab_id: 1,
            cwd: Some(PathBuf::from("/tmp")),
            cmd: Cmd::agent(Agent::Claude, AgentState::NeedsAttention),
            indicator: TabIndicator::Red,
            git_stat: GitStat {
                insertions: 1,
                deletions: 2,
                new_files: 3,
                is_worktree: true,
            },
        };

        let content = entry.to_string();
        assert2::assert!(let Ok(parsed) = TabStateEntry::try_from((1, content.as_str())));
        pretty_assertions::assert_eq!(parsed, entry);
    }

    #[test]
    fn test_tab_state_entry_legacy_parse_infers_indicator_from_cmd() {
        let content = "/tmp\nclaude\nneeds_attention\n1\n2\n3\n1\n--\n";

        assert2::assert!(let Ok(parsed) = TabStateEntry::try_from((1, content)));
        pretty_assertions::assert_eq!(parsed.indicator, TabIndicator::Red);
    }

    #[test]
    fn test_tab_state_entry_legacy_parse_infers_none_indicator_for_running_cmd() {
        let content = "/tmp\n--\n--\n1\n2\n3\n1\ncargo\n";

        assert2::assert!(let Ok(parsed) = TabStateEntry::try_from((1, content)));
        pretty_assertions::assert_eq!(parsed.indicator, TabIndicator::None);
    }

    #[test]
    fn test_tab_state_entry_explicit_empty_indicator_for_running_cmd_normalizes_to_none() {
        let content = "/tmp\n--\n--\nempty\n1\n2\n3\n1\ncargo\n";

        assert2::assert!(let Ok(parsed) = TabStateEntry::try_from((1, content)));
        pretty_assertions::assert_eq!(parsed.indicator, TabIndicator::None);
    }

    #[test]
    fn test_short_path_under_home() {
        let home = Path::new("/home/user");
        pretty_assertions::assert_eq!(
            super::short_path(Path::new("/home/user/src/pkg/myproject"), home),
            "~/s/p/myproject"
        );
    }

    #[test]
    fn test_short_path_many_dirs() {
        let home = Path::new("/home/user");
        pretty_assertions::assert_eq!(
            super::short_path(Path::new("/home/user/one/two/three/four/five"), home),
            "~/o/t/t/f/five"
        );
    }

    #[test]
    fn test_short_path_outside_home() {
        let home = Path::new("/home/user");
        pretty_assertions::assert_eq!(super::short_path(Path::new("/opt/pkg/foo"), home), "/o/p/foo");
    }

    #[test]
    fn test_cmd_acknowledge_needs_attention_transitions_to_acknowledged() {
        let mut cmd = Cmd::agent(Agent::Codex, AgentState::NeedsAttention);

        assert2::assert!(cmd.needs_attention());
        assert2::assert!(cmd.acknowledge());
        pretty_assertions::assert_eq!(cmd, Cmd::agent(Agent::Codex, AgentState::Acknowledged));
        assert2::assert!(!cmd.needs_attention());
        assert2::assert!(!cmd.acknowledge());
    }
}
