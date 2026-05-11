use std::convert::TryFrom;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::path::PathBuf;
use std::str::FromStr;

pub use ytil_agents::ParseError;
use ytil_agents::agent::Agent;
use ytil_agents::agent::AgentEventKind;
use ytil_agents::agent::AgentEventPayload;
pub use ytil_tui::short_path;

pub const AGENTS_PIPE: &str = "agg-agent";
pub const EMPTY_FIELD: &str = "--";
const GIT_STAT_FIELD_COUNT: usize = 9;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitStat {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub last_commit: Option<LastCommit>,
    pub insertions: usize,
    pub deletions: usize,
    pub new_files: usize,
    pub is_worktree: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LastCommit {
    pub short_sha: String,
    pub age: String,
    pub summary: String,
}

impl Display for GitStat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let path = encode_git_stat_field(&self.path.display().to_string());
        let branch = self.branch.as_deref().map(encode_git_stat_field).unwrap_or_default();
        let last_commit = self
            .last_commit
            .as_ref()
            .map_or_else(|| "\n\n".to_string(), ToString::to_string);
        write!(
            f,
            "{path}\n{branch}\n{}\n{}\n{}\n{}\n{last_commit}",
            self.insertions,
            self.deletions,
            self.new_files,
            u8::from(self.is_worktree)
        )
    }
}

impl Display for LastCommit {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let short_sha = encode_git_stat_field(&self.short_sha);
        let age = encode_git_stat_field(&self.age);
        let summary = encode_git_stat_field(&self.summary);
        write!(f, "{short_sha}\n{age}\n{summary}")
    }
}

impl FromStr for GitStat {
    type Err = ParseError;

    fn from_str(record: &str) -> Result<Self, Self::Err> {
        let mut fields = record.split('\n');
        let mut next = |name| fields.next().ok_or(ParseError::Missing(name));

        let path = PathBuf::from(decode_git_stat_field(next("path")?, "path")?);
        let branch = decode_git_stat_field(next("branch")?, "branch")?;
        let insertions = parse_usize(next("ins")?, "ins")?;
        let deletions = parse_usize(next("del")?, "del")?;
        let new_files = parse_usize(next("new")?, "new")?;
        let is_worktree = parse_bool(next("wt")?, "wt")?;
        let short_sha_field = next("last_commit_short_sha")?;
        let age_field = next("last_commit_age")?;
        let summary_field = next("last_commit_summary")?;
        let last_commit = if short_sha_field.is_empty() && age_field.is_empty() && summary_field.is_empty() {
            None
        } else {
            Some(format!("{short_sha_field}\n{age_field}\n{summary_field}").parse()?)
        };
        if fields.next().is_some() {
            return Err(ParseError::Invalid {
                field: "git_stat",
                value: "too many fields".to_string(),
            });
        }

        Ok(Self {
            path,
            branch: (!branch.is_empty()).then_some(branch),
            last_commit,
            insertions,
            deletions,
            new_files,
            is_worktree,
        })
    }
}

impl FromStr for LastCommit {
    type Err = ParseError;

    fn from_str(record: &str) -> Result<Self, Self::Err> {
        let mut fields = record.split('\n');
        let mut next = |name| fields.next().ok_or(ParseError::Missing(name));

        let short_sha = decode_git_stat_field(next("last_commit_short_sha")?, "last_commit_short_sha")?;
        let age = decode_git_stat_field(next("last_commit_age")?, "last_commit_age")?;
        let summary = decode_git_stat_field(next("last_commit_summary")?, "last_commit_summary")?;
        if fields.next().is_some() {
            return Err(ParseError::Invalid {
                field: "last_commit",
                value: "too many fields".to_string(),
            });
        }
        if short_sha.is_empty() || age.is_empty() {
            return Err(ParseError::Invalid {
                field: "last_commit",
                value: "incomplete".to_string(),
            });
        }

        Ok(Self {
            short_sha,
            age,
            summary,
        })
    }
}

/// Parse one or more newline-field [`GitStat`] records.
///
/// # Errors
/// Returns [`ParseError`] if the record has an incomplete field count or any field is invalid.
pub fn parse_git_stat_records(output: &str) -> Result<Vec<GitStat>, ParseError> {
    if output.is_empty() {
        return Ok(Vec::new());
    }

    let fields = output.split('\n').collect::<Vec<_>>();
    if fields.len() % GIT_STAT_FIELD_COUNT != 0 {
        return Err(ParseError::Invalid {
            field: "git_stat",
            value: format!(
                "expected fields in chunks of {GIT_STAT_FIELD_COUNT}, got {}",
                fields.len()
            ),
        });
    }

    fields
        .chunks(GIT_STAT_FIELD_COUNT)
        .map(|fields| fields.join("\n").parse())
        .collect()
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

    pub const fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Agent {
                state: AgentState::Busy,
                ..
            }
        )
    }

    pub const fn needs_attention(&self) -> bool {
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
            AgentEventKind::Busy => Self::agent(value.agent, AgentState::Busy),
            AgentEventKind::Start | AgentEventKind::Idle => Self::agent(value.agent, AgentState::Acknowledged),
            AgentEventKind::Exit => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TabIndicator {
    #[default]
    NoAgent,
    Seen,
    Busy,
    Unseen,
}

impl TabIndicator {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoAgent => "no_agent",
            Self::Seen => "seen",
            Self::Busy => "busy",
            Self::Unseen => "unseen",
        }
    }

    /// Parse a serialized tab indicator.
    ///
    /// # Errors
    /// Returns [`ParseError`] when `s` is not a supported indicator value.
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        match s {
            "no_agent" => Ok(Self::NoAgent),
            "seen" => Ok(Self::Seen),
            "busy" => Ok(Self::Busy),
            "unseen" => Ok(Self::Unseen),
            _ => Err(ParseError::invalid("indicator", format!("{s:?}"))),
        }
    }

    pub const fn from_cmd(cmd: &Cmd) -> Self {
        match cmd {
            Cmd::Agent {
                state: AgentState::NeedsAttention,
                ..
            } => Self::Unseen,
            Cmd::Agent {
                state: AgentState::Busy,
                ..
            } => Self::Busy,
            Cmd::Agent { .. } => Self::Seen,
            Cmd::None | Cmd::Running(_) => Self::NoAgent,
        }
    }

    #[must_use]
    pub const fn normalize_for_cmd(self, cmd: &Cmd) -> Self {
        match (self, cmd) {
            (Self::NoAgent, Cmd::Agent { .. }) => Self::from_cmd(cmd),
            (Self::Seen, Cmd::None | Cmd::Running(_)) => Self::NoAgent,
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

    /// Parse a serialized agent state.
    ///
    /// # Errors
    /// Returns [`ParseError`] when `s` is not a supported agent state value.
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

impl Display for TabStateEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let cwd_label = self.cwd.as_ref().map(|p| p.display().to_string());
        let command_label = self.cmd.running_cmd();
        let agent_state = self.cmd.agent_state().map(AgentState::as_str);

        write!(
            f,
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            encode_opt(cwd_label.as_deref()),
            encode_opt(self.cmd.agent_name()),
            encode_opt(agent_state),
            self.indicator.as_str(),
            self.git_stat.insertions,
            self.git_stat.deletions,
            self.git_stat.new_files,
            u8::from(self.git_stat.is_worktree),
            encode_opt(command_label),
        )
    }
}

impl TryFrom<(usize, &str)> for TabStateEntry {
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
            Err(_) => (TabIndicator::NoAgent, parse_usize(indicator_or_ins, "ins")?, false),
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
                ..Default::default()
            },
        })
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

fn encode_git_stat_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out
}

fn decode_git_stat_field(value: &str, name: &'static str) -> Result<String, ParseError> {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err(ParseError::Invalid {
                field: name,
                value: "trailing escape".to_string(),
            });
        };
        match escaped {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            _ => {
                return Err(ParseError::Invalid {
                    field: name,
                    value: format!("invalid escape \\{escaped}"),
                });
            }
        }
    }
    Ok(out)
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

    use rstest::rstest;

    use super::*;

    #[test]
    fn test_git_stat_wire_roundtrip_when_fields_need_escaping_preserves_values() {
        let stat = GitStat {
            path: PathBuf::from("/tmp/re\\po\nx"),
            branch: Some("feat\tone\\two".to_string()),
            last_commit: Some(LastCommit {
                short_sha: "abc1234".to_string(),
                age: "2m".to_string(),
                summary: "fix picker\tbranch metadata".to_string(),
            }),
            insertions: 2,
            deletions: 1,
            new_files: 3,
            is_worktree: true,
        };

        let record = stat.to_string();
        assert2::assert!(let Ok(parsed) = record.parse::<GitStat>());

        pretty_assertions::assert_eq!(
            record,
            "/tmp/re\\\\po\\nx\nfeat\\tone\\\\two\n2\n1\n3\n1\nabc1234\n2m\nfix picker\\tbranch metadata"
        );
        pretty_assertions::assert_eq!(parsed, stat);
    }

    #[test]
    fn test_git_stat_wire_parse_when_branch_empty_returns_none() {
        assert2::assert!(let Ok(parsed) = "/tmp/repo\n\n0\n0\n0\n0\n\n\n".parse::<GitStat>());

        pretty_assertions::assert_eq!(
            parsed,
            GitStat {
                path: PathBuf::from("/tmp/repo"),
                branch: None,
                ..Default::default()
            }
        );
    }

    #[test]
    fn test_last_commit_wire_roundtrip_allows_empty_summary() {
        let commit = LastCommit {
            short_sha: "abc1234".to_string(),
            age: "2m".to_string(),
            summary: String::new(),
        };

        let record = commit.to_string();
        assert2::assert!(let Ok(parsed) = record.parse::<LastCommit>());

        pretty_assertions::assert_eq!(record, "abc1234\n2m\n");
        pretty_assertions::assert_eq!(parsed, commit);
    }

    #[test]
    fn test_last_commit_wire_parse_when_required_field_missing_returns_error() {
        assert2::assert!(let Err(err) = "\n2m\nsummary".parse::<LastCommit>());

        pretty_assertions::assert_eq!(
            err,
            ParseError::Invalid {
                field: "last_commit",
                value: "incomplete".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_git_stat_records_when_multiple_records_returns_all() {
        let first = GitStat {
            path: PathBuf::from("/tmp/one"),
            branch: Some("main".to_string()),
            ..Default::default()
        };
        let second = GitStat {
            path: PathBuf::from("/tmp/two"),
            branch: Some("next".to_string()),
            last_commit: Some(LastCommit {
                short_sha: "def5678".to_string(),
                age: "1d".to_string(),
                summary: "ship newline format".to_string(),
            }),
            insertions: 4,
            is_worktree: true,
            ..Default::default()
        };
        let output = format!("{first}\n{second}");

        assert2::assert!(let Ok(parsed) = parse_git_stat_records(&output));

        pretty_assertions::assert_eq!(parsed, vec![first, second]);
    }

    #[test]
    fn test_tab_state_entry_serialization_roundtrip_when_entry_valid_preserves_values() {
        let entry = TabStateEntry {
            tab_id: 1,
            cwd: Some(PathBuf::from("/tmp")),
            cmd: Cmd::agent(Agent::Claude, AgentState::NeedsAttention),
            indicator: TabIndicator::Unseen,
            git_stat: GitStat {
                insertions: 1,
                deletions: 2,
                new_files: 3,
                is_worktree: true,
                ..Default::default()
            },
        };

        let content = entry.to_string();
        assert2::assert!(let Ok(parsed) = TabStateEntry::try_from((1, content.as_str())));
        pretty_assertions::assert_eq!(parsed, entry);
    }

    #[rstest]
    #[case("no_agent", TabIndicator::NoAgent)]
    #[case("seen", TabIndicator::Seen)]
    #[case("busy", TabIndicator::Busy)]
    #[case("unseen", TabIndicator::Unseen)]
    fn test_tab_indicator_parse_when_value_is_semantic_returns_expected(
        #[case] value: &str,
        #[case] expected: TabIndicator,
    ) {
        assert2::assert!(let Ok(parsed) = TabIndicator::parse(value));
        pretty_assertions::assert_eq!(parsed, expected);
    }

    #[test]
    fn test_tab_state_entry_legacy_parse_infers_indicator_from_cmd() {
        let content = "/tmp\nclaude\nneeds_attention\n1\n2\n3\n1\n--\n";

        assert2::assert!(let Ok(parsed) = TabStateEntry::try_from((1, content)));
        pretty_assertions::assert_eq!(parsed.indicator, TabIndicator::Unseen);
    }

    #[test]
    fn test_tab_state_entry_legacy_parse_infers_no_agent_indicator_for_running_cmd() {
        let content = "/tmp\n--\n--\n1\n2\n3\n1\ncargo\n";

        assert2::assert!(let Ok(parsed) = TabStateEntry::try_from((1, content)));
        pretty_assertions::assert_eq!(parsed.indicator, TabIndicator::NoAgent);
    }

    #[test]
    fn test_tab_state_entry_explicit_seen_indicator_for_running_cmd_normalizes_to_no_agent() {
        let content = "/tmp\n--\n--\nseen\n1\n2\n3\n1\ncargo\n";

        assert2::assert!(let Ok(parsed) = TabStateEntry::try_from((1, content)));
        pretty_assertions::assert_eq!(parsed.indicator, TabIndicator::NoAgent);
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
