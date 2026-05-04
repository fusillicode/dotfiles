use std::fmt::Display;
use std::path::Path;
use std::path::PathBuf;

use strum::EnumIter;

use crate::ParseError;

pub mod session;
#[cfg(not(target_arch = "wasm32"))]
pub mod session_loader;
pub mod session_parser;

#[derive(Clone, Copy, Debug, EnumIter, Eq, PartialEq)]
pub enum Agent {
    Claude,
    Codex,
    Cursor,
    Gemini,
    Opencode,
}

impl Agent {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
        }
    }

    pub const fn short_name(self) -> &'static str {
        match self {
            Self::Claude => "cl",
            Self::Codex => "cx",
            Self::Cursor => "cu",
            Self::Gemini => "gm",
            Self::Opencode => "oc",
        }
    }

    pub const fn default_config(self) -> &'static str {
        match self {
            Self::Cursor => r#"{"version":1,"hooks":{}}"#,
            Self::Claude | Self::Codex => r#"{"hooks":{}}"#,
            Self::Gemini => r#"{"hooksConfig":{"enabled":true},"hooks":{}}"#,
            Self::Opencode => "{}",
        }
    }

    pub const fn root_path(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude"],
            Self::Cursor => &[".cursor"],
            Self::Codex => &[".codex"],
            Self::Gemini => &[".gemini"],
            Self::Opencode => &[".config", "opencode"],
        }
    }

    pub const fn sessions_root_path(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude", "projects"],
            Self::Cursor => &[".cursor", "chats"],
            Self::Codex => &[".codex", "sessions"],
            Self::Gemini | Self::Opencode => Self::root_path(self),
        }
    }

    pub const fn config_path(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude", "settings.json"],
            Self::Cursor => &[".cursor", "hooks.json"],
            Self::Codex => &[".codex", "hooks.json"],
            Self::Gemini => &[".gemini", "settings.json"],
            Self::Opencode => &[],
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
                // `PermissionRequest` runs before Codex falls back to user or
                // guardian approval, so it is not a reliable "waiting for user"
                // signal. Keep it busy to avoid false red indicators.
                ("SessionStart", AgentEventKind::Start),
                ("UserPromptSubmit", AgentEventKind::Busy),
                ("PreToolUse", AgentEventKind::Busy),
                ("PostToolUse", AgentEventKind::Busy),
                ("PermissionRequest", AgentEventKind::Busy),
                ("Stop", AgentEventKind::Idle),
            ],
            Self::Gemini => &[
                ("SessionStart", AgentEventKind::Start),
                ("BeforeAgent", AgentEventKind::Busy),
                ("BeforeModel", AgentEventKind::Busy),
                ("BeforeToolSelection", AgentEventKind::Busy),
                ("BeforeTool", AgentEventKind::Busy),
                ("Notification", AgentEventKind::Idle),
                ("AfterAgent", AgentEventKind::Idle),
                ("SessionEnd", AgentEventKind::Exit),
            ],
            Self::Opencode => &[],
        }
    }

    /// Parse a lowercase agent identifier.
    ///
    /// # Errors
    /// Returns [`ParseError`] when `s` is not a supported agent name.
    pub fn from_name(s: &str) -> Result<Self, ParseError> {
        match s {
            "claude" => Ok(Self::Claude),
            "cursor" => Ok(Self::Cursor),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            "opencode" => Ok(Self::Opencode),
            _ => Err(ParseError::Invalid {
                field: "agent",
                value: format!("{s:?}"),
            }),
        }
    }

    pub const fn priority(self) -> u8 {
        match self {
            Self::Claude => 0,
            Self::Codex => 1,
            Self::Cursor => 2,
            Self::Gemini => 3,
            Self::Opencode => 4,
        }
    }

    pub fn detect(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        if lower.contains("claude") {
            Some(Self::Claude)
        } else if lower.contains("cursor") {
            Some(Self::Cursor)
        } else if lower.contains("codex") {
            Some(Self::Codex)
        } else if lower.contains("gemini") {
            Some(Self::Gemini)
        } else if lower.contains("opencode") {
            Some(Self::Opencode)
        } else {
            None
        }
    }
}

impl Display for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let repr = match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
            Self::Gemini => "Gemini",
            Self::Opencode => "Opencode",
        };
        write!(f, "{repr}")
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

    /// Parse an agent event payload kind.
    ///
    /// # Errors
    /// Returns [`ParseError`] when `s` is not one of the supported event kinds.
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentEventPayload {
    pub pane_id: u32,
    pub agent: Agent,
    pub kind: AgentEventKind,
}

impl AgentEventPayload {
    /// Parse a Zellij pipe payload into a typed agent event.
    ///
    /// # Errors
    /// Returns [`ParseError`] when the pane id, agent, or event kind is invalid.
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentIcon {
    pub cache_key: &'static str,
}

impl AgentIcon {
    pub fn dir(home_dir: &Path) -> PathBuf {
        home_dir.join(".cache").join("yog").join("agents")
    }

    pub fn path(self, home_dir: &Path) -> PathBuf {
        Self::dir(home_dir).join(format!("{}.png", self.cache_key))
    }
}

impl From<Agent> for AgentIcon {
    fn from(agent: Agent) -> Self {
        match agent {
            Agent::Claude => Self { cache_key: "claude" },
            Agent::Codex => Self { cache_key: "codex" },
            Agent::Cursor => Self { cache_key: "cursor" },
            Agent::Gemini => Self { cache_key: "gemini" },
            Agent::Opencode => Self { cache_key: "opencode" },
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("claude", Ok(Agent::Claude))]
    #[case("cursor", Ok(Agent::Cursor))]
    #[case("codex", Ok(Agent::Codex))]
    #[case("gemini", Ok(Agent::Gemini))]
    #[case("opencode", Ok(Agent::Opencode))]
    #[case("unknown", Err("invalid agent: \"unknown\"".to_string()))]
    fn test_agent_from_name_works_as_expected(#[case] name: &str, #[case] expected: Result<Agent, String>) {
        let actual = Agent::from_name(name).map_err(|e| e.to_string());
        pretty_assertions::assert_eq!(actual, expected);
    }

    #[rstest]
    #[case("Claude-3.5-Sonnet", Some(Agent::Claude))]
    #[case("Cursor-IDE", Some(Agent::Cursor))]
    #[case("GitHub-Codex", Some(Agent::Codex))]
    #[case("Gemini-1.5-Pro", Some(Agent::Gemini))]
    #[case("OpenCode-Agent", Some(Agent::Opencode))]
    #[case("Vim", None)]
    fn test_agent_detect_works_as_expected(#[case] name: &str, #[case] expected: Option<Agent>) {
        pretty_assertions::assert_eq!(Agent::detect(name), expected);
    }

    #[test]
    fn test_agent_gemini_hook_events_match_supported_lifecycle() {
        let expected = [
            ("SessionStart", AgentEventKind::Start),
            ("BeforeAgent", AgentEventKind::Busy),
            ("BeforeModel", AgentEventKind::Busy),
            ("BeforeToolSelection", AgentEventKind::Busy),
            ("BeforeTool", AgentEventKind::Busy),
            ("Notification", AgentEventKind::Idle),
            ("AfterAgent", AgentEventKind::Idle),
            ("SessionEnd", AgentEventKind::Exit),
        ];

        pretty_assertions::assert_eq!(Agent::Gemini.hook_events(), expected);
    }

    #[test]
    fn test_agent_codex_permission_request_remains_busy() {
        let permission_request_kind = Agent::Codex
            .hook_events()
            .iter()
            .find_map(|(event, kind)| (*event == "PermissionRequest").then_some(*kind));

        pretty_assertions::assert_eq!(permission_request_kind, Some(AgentEventKind::Busy));
    }

    #[rstest]
    #[case(Agent::Claude, "claude")]
    #[case(Agent::Cursor, "cursor")]
    #[case(Agent::Codex, "codex")]
    #[case(Agent::Gemini, "gemini")]
    #[case(Agent::Opencode, "opencode")]
    fn test_agent_icon_from_agent_returns_agent_icon(#[case] agent: Agent, #[case] cache_key: &str) {
        let icon = AgentIcon::from(agent);

        pretty_assertions::assert_eq!(icon.cache_key, cache_key);
    }

    #[test]
    fn test_agent_icon_path_uses_yog_agents_dir() {
        let icon = AgentIcon::from(Agent::Codex);

        pretty_assertions::assert_eq!(
            icon.path(Path::new("/home/me")),
            PathBuf::from("/home/me/.cache/yog/agents/codex.png")
        );
    }
}
