use std::path::PathBuf;

pub const AGENTS_PIPE: &str = "agm-agent";
pub const EMPTY_FIELD: &str = "--";

#[derive(Debug)]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GitStat {
    pub insertions: usize,
    pub deletions: usize,
    pub new_files: usize,
    pub is_worktree: bool,
}

impl GitStat {
    pub fn parse_line(line: &str) -> Result<(PathBuf, Self), ParseError> {
        let mut parts = line.rsplitn(5, ' ');
        let is_worktree = parts
            .next()
            .ok_or(ParseError::Missing("worktree field"))
            .and_then(|v| {
                v.parse::<u8>().map_err(|_| ParseError::Invalid {
                    field: "worktree",
                    value: format!("{v:?}"),
                })
            })?
            != 0;
        let new_files = parts
            .next()
            .ok_or(ParseError::Missing("new_files field"))
            .and_then(|v| {
                v.parse().map_err(|_| ParseError::Invalid {
                    field: "new_files",
                    value: format!("{v:?}"),
                })
            })?;
        let deletions = parts
            .next()
            .ok_or(ParseError::Missing("deletions field"))
            .and_then(|v| {
                v.parse().map_err(|_| ParseError::Invalid {
                    field: "deletions",
                    value: format!("{v:?}"),
                })
            })?;
        let insertions = parts
            .next()
            .ok_or(ParseError::Missing("insertions field"))
            .and_then(|v| {
                v.parse().map_err(|_| ParseError::Invalid {
                    field: "insertions",
                    value: format!("{v:?}"),
                })
            })?;
        let path = PathBuf::from(parts.next().ok_or(ParseError::Missing("path"))?);
        Ok((
            path,
            Self {
                insertions,
                deletions,
                new_files,
                is_worktree,
            },
        ))
    }
}

impl std::fmt::Display for GitStat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.insertions,
            self.deletions,
            self.new_files,
            u8::from(self.is_worktree),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Agent {
    Claude,
    Codex,
    Cursor,
    Opencode,
}

impl Agent {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Opencode => "opencode",
        }
    }

    pub const fn default_config(self) -> &'static str {
        match self {
            Self::Claude => r#"{"hooks":{}}"#,
            Self::Cursor => r#"{"version":1,"hooks":{}}"#,
            Self::Codex => r#"{"hooks":{}}"#,
            Self::Opencode => "{}",
        }
    }

    pub const fn config_path(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude", "settings.json"],
            Self::Cursor => &[".cursor", "hooks.json"],
            Self::Codex => &[".codex", "hooks.json"],
            Self::Opencode => &[".config", "opencode", "plugins", "agm.ts"],
        }
    }

    pub const fn hook_events(self) -> &'static [(&'static str, AgentEventKind)] {
        match self {
            Self::Claude => &[
                ("SessionStart", AgentEventKind::Start),
                ("UserPromptSubmit", AgentEventKind::Busy),
                ("Stop", AgentEventKind::Idle),
                ("SessionEnd", AgentEventKind::Exit),
            ],
            Self::Cursor => &[
                ("sessionStart", AgentEventKind::Start),
                ("beforeSubmitPrompt", AgentEventKind::Busy),
                ("stop", AgentEventKind::Idle),
                ("sessionEnd", AgentEventKind::Exit),
            ],
            Self::Codex => &[
                ("SessionStart", AgentEventKind::Start),
                ("UserPromptSubmit", AgentEventKind::Busy),
                // Fires while tools run (Codex may not emit another UserPromptSubmit until the next turn).
                ("PreToolUse", AgentEventKind::Busy),
                ("Stop", AgentEventKind::Idle),
            ],
            Self::Opencode => &[],
        }
    }

    pub fn from_name(s: &str) -> Result<Self, ParseError> {
        match s {
            "claude" => Ok(Self::Claude),
            "cursor" => Ok(Self::Cursor),
            "codex" => Ok(Self::Codex),
            "opencode" => Ok(Self::Opencode),
            _ => Err(ParseError::Invalid {
                field: "agent",
                value: format!("{s:?}"),
            }),
        }
    }

    pub fn hook_command(self, kind: AgentEventKind) -> String {
        // Codex (and similar) hook runners write JSON to the hook process stdin. `zellij pipe` only
        // reads stdin when the payload argument is omitted, so that data would never be consumed and
        // the hook can block or fail—then the plugin never receives events. Drain stdin first.
        let pipe = format!(
            "zellij pipe --name {AGENTS_PIPE} --args \"pane_id=$ZELLIJ_PANE_ID,agent={}\" -- {} >/dev/null 2>&1 || true",
            self.name(),
            kind.as_str()
        );
        format!("cat >/dev/null 2>&1 || true; {pipe}")
    }

    /// Higher means more specific — Cursor hosts Claude, so it wins when both match.
    pub const fn priority(self) -> u8 {
        match self {
            Self::Claude => 0,
            Self::Codex => 1,
            Self::Cursor => 2,
            Self::Opencode => 3,
        }
    }

    /// Fuzzy match — checks if the lowercased name contains an agent identifier.
    pub fn detect(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        if lower.contains("claude") {
            Some(Self::Claude)
        } else if lower.contains("cursor") {
            Some(Self::Cursor)
        } else if lower.contains("codex") {
            Some(Self::Codex)
        } else if lower.contains("opencode") {
            Some(Self::Opencode)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentEventKind {
    Start,
    Busy,
    Idle,
    Exit,
}

impl AgentEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Busy => "busy",
            Self::Idle => "idle",
            Self::Exit => "exit",
        }
    }

    pub fn parse(s: &str) -> Result<Self, ParseError> {
        match s.trim() {
            "start" => Ok(Self::Start),
            "busy" => Ok(Self::Busy),
            "idle" => Ok(Self::Idle),
            "exit" => Ok(Self::Exit),
            _ => Err(ParseError::Invalid {
                field: "event kind",
                value: format!("{s:?}"),
            }),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentEventPayload {
    pub pane_id: u32,
    pub agent: Agent,
    pub kind: AgentEventKind,
}

impl AgentEventPayload {
    pub fn parse(pane_id: &str, agent: &str, payload: &str) -> Result<Self, ParseError> {
        let pane_id = pane_id.trim().parse().map_err(|_| ParseError::Invalid {
            field: "pane_id",
            value: format!("{pane_id:?}"),
        })?;
        let agent = Agent::from_name(agent.trim())?;
        let kind = AgentEventKind::parse(payload)?;
        Ok(Self { pane_id, agent, kind })
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

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("/home/user/project 10 5 2 1", "/home/user/project", 10, 5, 2, true)]
    #[case("/home/user/my project 10 5 2 0", "/home/user/my project", 10, 5, 2, false)]
    fn git_stat_parse_line_works_as_expected(
        #[case] line: &str,
        #[case] expected_path: &str,
        #[case] insertions: usize,
        #[case] deletions: usize,
        #[case] new_files: usize,
        #[case] is_worktree: bool,
    ) {
        let expected_stat = GitStat {
            insertions,
            deletions,
            new_files,
            is_worktree,
        };
        assert2::assert!(let Ok((path, stat)) = GitStat::parse_line(line));
        pretty_assertions::assert_eq!((path, stat), (PathBuf::from(expected_path), expected_stat));
    }

    #[rstest]
    #[case("claude", Ok(Agent::Claude))]
    #[case("cursor", Ok(Agent::Cursor))]
    #[case("codex", Ok(Agent::Codex))]
    #[case("opencode", Ok(Agent::Opencode))]
    #[case("unknown", Err("invalid agent: \"unknown\"".to_string()))]
    fn agent_from_name_works_as_expected(#[case] name: &str, #[case] expected: Result<Agent, String>) {
        let actual = Agent::from_name(name).map_err(|e| e.to_string());
        pretty_assertions::assert_eq!(actual, expected);
    }

    #[rstest]
    #[case("Claude-3.5-Sonnet", Some(Agent::Claude))]
    #[case("Cursor-IDE", Some(Agent::Cursor))]
    #[case("GitHub-Codex", Some(Agent::Codex))]
    #[case("OpenCode-Agent", Some(Agent::Opencode))]
    #[case("Vim", None)]
    fn agent_detect_works_as_expected(#[case] name: &str, #[case] expected: Option<Agent>) {
        pretty_assertions::assert_eq!(Agent::detect(name), expected);
    }

    #[rstest]
    #[case(Agent::Claude)]
    #[case(Agent::Cursor)]
    #[case(Agent::Codex)]
    #[case(Agent::Opencode)]
    fn hook_command_never_fails_when_zellij_unavailable(#[case] agent: Agent) {
        let cmd = agent.hook_command(AgentEventKind::Busy);
        assert2::assert!(cmd.contains("cat >/dev/null 2>&1 || true;"));
        assert2::assert!(cmd.contains("zellij pipe --name agm-agent"));
        assert2::assert!(cmd.contains(">/dev/null 2>&1 || true"));
    }

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
}
