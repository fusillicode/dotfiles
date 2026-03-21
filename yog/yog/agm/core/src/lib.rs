use std::path::PathBuf;

pub const PIPE_NAME: &str = "agm-agent";
pub const EMPTY_FIELD: &str = "--";

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
            Self::Codex => "{}",
            Self::Opencode => "{}",
        }
    }

    pub const fn config_path(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude", "settings.json"],
            Self::Cursor => &[".cursor", "hooks.json"],
            Self::Codex => &[],
            Self::Opencode => &[".config", "opencode", "plugins", "agm.js"],
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
            Self::Opencode => &[],
        }
    }

    pub fn from_name(s: &str) -> Result<Self, ParseError> {
        match s {
            "claude" => Ok(Self::Claude),
            "cursor" => Ok(Self::Cursor),
            "codex" => Ok(Self::Codex),
            "opencode" => Ok(Self::Opencode),
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
            _ => None,
        }
    }

    pub fn running_cmd(&self) -> Option<&str> {
        match self {
            Self::Running(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_agent(&self) -> bool {
        matches!(self, Self::IdleAgent(_) | Self::BusyAgent(_))
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

impl From<&AgentEvent> for Cmd {
    fn from(value: &AgentEvent) -> Self {
        match value.kind {
            AgentEventKind::Start => Self::IdleAgent(value.agent),
            AgentEventKind::Busy => Self::BusyAgent(value.agent),
            AgentEventKind::Idle => Self::IdleAgent(value.agent),
            AgentEventKind::Exit => Self::None,
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

pub fn state_base_dir() -> PathBuf {
    #[cfg(target_os = "wasi")]
    {
        PathBuf::from("/cache")
    }
    #[cfg(not(target_os = "wasi"))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        home.join(".local").join("share").join("agm")
    }
}

pub fn session_state_dir(session: &str) -> PathBuf {
    state_base_dir().join(session)
}

pub fn state_file_path(session: &str, tab_id: usize) -> PathBuf {
    session_state_dir(session).join(format!("tab-{tab_id}"))
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

pub struct TabStateEntry {
    pub tab_id: usize,
    pub cwd: Option<PathBuf>,
    pub cmd: Cmd,
    pub git_stat: GitStat,
}

impl TabStateEntry {
    pub fn to_file_content(&self) -> String {
        let cwd_s = self.cwd.as_ref().map(|p| p.display().to_string());
        let cmd_s = self.cmd.running_cmd();

        format!(
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

    pub fn parse_file_content(tab_id: usize, content: &str) -> Result<Self, ParseError> {
        let mut l = content.lines();
        let mut next = |name| l.next().ok_or_else(|| ParseError::new(format!("missing {name}")));

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

/// List all persisted tab states for a session.
pub fn read_all_state_files(session: &str) -> Vec<TabStateEntry> {
    let dir = session_state_dir(session);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let Some(id_str) = name_str.strip_prefix("tab-") else {
            continue;
        };
        if id_str.contains('.') {
            continue;
        }
        let Ok(tab_id) = id_str.parse::<usize>() else { continue };
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Ok(entry) = TabStateEntry::parse_file_content(tab_id, &content) {
            out.push(entry);
        }
    }
    out
}

/// Atomically write a state file (write .tmp then rename).
pub fn write_state_file(session: &str, tab_id: usize, content: &str) -> std::io::Result<()> {
    let path = state_file_path(session, tab_id);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, &path)
}

pub fn remove_state_file(session: &str, tab_id: usize) {
    let _ = std::fs::remove_file(state_file_path(session, tab_id));
}

pub fn clean_state_dir(session: &str) {
    let _ = std::fs::remove_dir_all(session_state_dir(session));
}

fn parse_bool(s: &str, name: &str) -> Result<bool, ParseError> {
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(ParseError::new(format!("invalid {name} flag: {s:?}"))),
    }
}

fn parse_usize(s: &str, name: &str) -> Result<usize, ParseError> {
    s.parse().map_err(|_| ParseError::new(format!("invalid {name}: {s:?}")))
}
