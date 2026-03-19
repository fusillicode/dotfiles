use std::path::PathBuf;

pub const PIPE_NAME: &str = "agm-agent";

#[derive(Debug)]
pub struct ParseError(String);

impl ParseError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
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
            .ok_or_else(|| ParseError::new("missing worktree field"))
            .and_then(|v| {
                v.parse::<u8>()
                    .map_err(|_| ParseError::new(format!("invalid worktree flag {v:?}")))
            })?
            != 0;
        let new_files = parts
            .next()
            .ok_or_else(|| ParseError::new("missing new_files field"))
            .and_then(|v| {
                v.parse()
                    .map_err(|_| ParseError::new(format!("invalid new_files {v:?}")))
            })?;
        let deletions = parts
            .next()
            .ok_or_else(|| ParseError::new("missing deletions field"))
            .and_then(|v| {
                v.parse()
                    .map_err(|_| ParseError::new(format!("invalid deletions {v:?}")))
            })?;
        let insertions = parts
            .next()
            .ok_or_else(|| ParseError::new("missing insertions field"))
            .and_then(|v| {
                v.parse()
                    .map_err(|_| ParseError::new(format!("invalid insertions {v:?}")))
            })?;
        let path = PathBuf::from(parts.next().ok_or_else(|| ParseError::new("missing path field"))?);
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

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Agent {
    Claude,
    Codex,
    Cursor,
}

impl Agent {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }

    pub const fn default_config(self) -> &'static str {
        match self {
            Self::Claude => r#"{"hooks":{}}"#,
            Self::Cursor => r#"{"version":1,"hooks":{}}"#,
            Self::Codex => "{}",
        }
    }

    pub const fn config_path(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude", "settings.json"],
            Self::Cursor => &[".cursor", "hooks.json"],
            Self::Codex => &[],
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
            Self::Codex => &[],
        }
    }

    pub fn from_name(s: &str) -> Result<Self, ParseError> {
        match s {
            "claude" => Ok(Self::Claude),
            "cursor" => Ok(Self::Cursor),
            "codex" => Ok(Self::Codex),
            _ => Err(ParseError::new(format!("unknown agent {s:?}"))),
        }
    }

    pub fn hook_command(self, kind: AgentEventKind) -> String {
        format!("agm hook {} {}", self.name(), kind.as_str())
    }

    /// Higher means more specific — Cursor hosts Claude, so it wins when both match.
    pub const fn priority(self) -> u8 {
        match self {
            Self::Claude => 0,
            Self::Codex => 1,
            Self::Cursor => 2,
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
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
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
        match s {
            "start" => Ok(Self::Start),
            "busy" => Ok(Self::Busy),
            "idle" => Ok(Self::Idle),
            "exit" => Ok(Self::Exit),
            _ => Err(ParseError::new(format!("unknown event kind {s:?}"))),
        }
    }
}

pub struct AgentEvent {
    pub pane_id: u32,
    pub agent: Agent,
    pub kind: AgentEventKind,
}

impl AgentEvent {
    pub fn parse(pane_id: &str, agent: &str, payload: &str) -> Result<Self, ParseError> {
        let pane_id = pane_id
            .parse()
            .map_err(|_| ParseError::new(format!("invalid pane_id {pane_id:?}")))?;
        let agent = Agent::from_name(agent)?;
        let kind = AgentEventKind::parse(payload)?;
        Ok(Self { pane_id, agent, kind })
    }
}
