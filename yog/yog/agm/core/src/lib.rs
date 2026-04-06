pub mod agent;
pub mod git_stat;

use std::path::Path;
use std::path::PathBuf;

use crate::agent::Agent;
use crate::agent::AgentEventKind;
use crate::agent::AgentEventPayload;
use crate::git_stat::GitStat;

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Cmd {
    #[default]
    None,
    Running(String),
    IdleAgent(Agent),
    BusyAgent(Agent),
}

impl Cmd {
    pub fn agent_name(&self) -> Option<&'static str> {
        match self {
            Self::IdleAgent(agent) | Self::BusyAgent(agent) => Some(agent.name()),
            Self::None | Self::Running(_) => None,
        }
    }

    pub fn running_cmd(&self) -> Option<&str> {
        match self {
            Self::Running(s) => Some(s),
            Self::None | Self::IdleAgent(_) | Self::BusyAgent(_) => None,
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Self::BusyAgent(_))
    }

    pub fn from_parts(agent: Option<Agent>, agent_busy: bool, command: Option<String>) -> Self {
        let Some(agent) = agent else {
            return command.map_or(Self::None, Self::Running);
        };
        (if agent_busy { Self::BusyAgent } else { Self::IdleAgent })(agent)
    }

    pub fn into_parts(self) -> (Option<Agent>, bool, Option<String>) {
        match self {
            Self::None => (None, false, None),
            Self::Running(cmd) => (None, false, Some(cmd)),
            Self::IdleAgent(agent) => (Some(agent), false, None),
            Self::BusyAgent(agent) => (Some(agent), true, None),
        }
    }
}

impl From<&AgentEventPayload> for Cmd {
    fn from(value: &AgentEventPayload) -> Self {
        match value.kind {
            AgentEventKind::Start => Self::IdleAgent(value.agent),
            AgentEventKind::Busy => Self::BusyAgent(value.agent),
            AgentEventKind::Idle => Self::IdleAgent(value.agent),
            AgentEventKind::Exit => Self::None,
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

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct TabStateEntry {
    pub tab_id: usize,
    pub cwd: Option<PathBuf>,
    pub cmd: Cmd,
    pub git_stat: GitStat,
}

impl std::fmt::Display for TabStateEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cwd_s = self.cwd.as_ref().map(|p| p.display().to_string());
        let cmd_s = self.cmd.running_cmd();

        write!(
            f,
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            encode_opt(cwd_s.as_deref()),
            encode_opt(self.cmd.agent_name()),
            u8::from(self.cmd.is_busy()),
            self.git_stat.insertions,
            self.git_stat.deletions,
            self.git_stat.new_files,
            u8::from(self.git_stat.is_worktree),
            encode_opt(cmd_s),
        )
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
        let agent_busy = parse_bool(next("busy")?, "busy")?;
        let insertions = parse_usize(next("ins")?, "ins")?;
        let deletions = parse_usize(next("del")?, "del")?;
        let new_files = parse_usize(next("new")?, "new")?;
        let is_worktree = parse_bool(next("wt")?, "wt")?;
        let command = decode_opt(next("cmd")?);

        Ok(Self {
            tab_id,
            cwd,
            cmd: Cmd::from_parts(agent, agent_busy, command),
            git_stat: GitStat {
                insertions,
                deletions,
                new_files,
                is_worktree,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;
    use std::path::Path;

    use super::*;

    #[test]
    fn tab_state_entry_serialization_roundtrip_works_as_expected() {
        let entry = TabStateEntry {
            tab_id: 1,
            cwd: Some(PathBuf::from("/tmp")),
            cmd: Cmd::BusyAgent(Agent::Claude),
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
    fn short_path_under_home() {
        let home = Path::new("/home/user");
        pretty_assertions::assert_eq!(
            super::short_path(Path::new("/home/user/src/pkg/myproject"), home),
            "~/s/p/myproject"
        );
    }

    #[test]
    fn short_path_many_dirs() {
        let home = Path::new("/home/user");
        pretty_assertions::assert_eq!(
            super::short_path(Path::new("/home/user/one/two/three/four/five"), home),
            "~/o/t/t/f/five"
        );
    }

    #[test]
    fn short_path_outside_home() {
        let home = Path::new("/home/user");
        pretty_assertions::assert_eq!(super::short_path(Path::new("/opt/pkg/foo"), home), "/o/p/foo");
    }
}
