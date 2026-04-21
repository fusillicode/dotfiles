use std::fmt::Display;

use strum::EnumIter;

use crate::ParseError;

pub mod session;
#[cfg(not(target_arch = "wasm32"))]
pub mod session_loader;
pub mod session_parser;

pub const AGENTS_PIPE: &str = "agm-agent";

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

    pub const fn default_config(self) -> &'static str {
        match self {
            Self::Claude => r#"{"hooks":{}}"#,
            Self::Cursor => r#"{"version":1,"hooks":{}}"#,
            Self::Codex => r#"{"hooks":{}}"#,
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
            Self::Gemini => Self::root_path(self),
            Self::Opencode => Self::root_path(self),
        }
    }

    pub const fn config_path(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &[".claude", "settings.json"],
            Self::Cursor => &[".cursor", "hooks.json"],
            Self::Codex => &[".codex", "hooks.json"],
            Self::Gemini => &[".gemini", "settings.json"],
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
                // Codex can signal approval waits via `PermissionRequest`, but it
                // still lacks a generic hook for arbitrary mid-turn clarification /
                // choice prompts. Those may remain busy until `Stop`.
                ("SessionStart", AgentEventKind::Start),
                ("UserPromptSubmit", AgentEventKind::Busy),
                ("PreToolUse", AgentEventKind::Busy),
                ("PermissionRequest", AgentEventKind::Idle),
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

    pub fn hook_name(self, event: &str) -> Option<&'static str> {
        match self {
            Self::Gemini => match event {
                "SessionStart" => Some("agm-gemini-session-start"),
                "BeforeAgent" => Some("agm-gemini-before-agent"),
                "BeforeModel" => Some("agm-gemini-before-model"),
                "BeforeToolSelection" => Some("agm-gemini-before-tool-selection"),
                "BeforeTool" => Some("agm-gemini-before-tool"),
                "Notification" => Some("agm-gemini-notification"),
                "AfterAgent" => Some("agm-gemini-after-agent"),
                "SessionEnd" => Some("agm-gemini-session-end"),
                _ => None,
            },
            _ => None,
        }
    }

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

    pub fn hook_command(self, kind: AgentEventKind) -> String {
        let pipe = format!(
            "zellij pipe --name {AGENTS_PIPE} --args \"pane_id=$ZELLIJ_PANE_ID,agent={}\" -- {} >/dev/null 2>&1 || true",
            self.name(),
            kind.as_str()
        );
        let echo = if matches!(self, Self::Gemini) {
            "; echo '{}'"
        } else {
            ""
        };
        format!("cat >/dev/null 2>&1 || true; {pipe}{echo}")
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
        write!(f, "{}", repr)
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

    #[rstest]
    #[case("SessionStart", Some("agm-gemini-session-start"))]
    #[case("BeforeAgent", Some("agm-gemini-before-agent"))]
    #[case("BeforeModel", Some("agm-gemini-before-model"))]
    #[case("BeforeToolSelection", Some("agm-gemini-before-tool-selection"))]
    #[case("BeforeTool", Some("agm-gemini-before-tool"))]
    #[case("Notification", Some("agm-gemini-notification"))]
    #[case("AfterAgent", Some("agm-gemini-after-agent"))]
    #[case("SessionEnd", Some("agm-gemini-session-end"))]
    #[case("Unknown", None)]
    fn test_agent_gemini_hook_name_works_as_expected(#[case] event: &str, #[case] expected: Option<&str>) {
        pretty_assertions::assert_eq!(Agent::Gemini.hook_name(event), expected);
        pretty_assertions::assert_eq!(Agent::Claude.hook_name(event), None);
    }

    #[rstest]
    #[case(Agent::Claude)]
    #[case(Agent::Cursor)]
    #[case(Agent::Codex)]
    #[case(Agent::Gemini)]
    #[case(Agent::Opencode)]
    fn test_hook_command_never_fails_when_zellij_unavailable(#[case] agent: Agent) {
        let cmd = agent.hook_command(AgentEventKind::Busy);
        assert2::assert!(cmd.contains("cat >/dev/null 2>&1 || true;"));
        assert2::assert!(cmd.contains("zellij pipe --name agm-agent"));
        assert2::assert!(cmd.contains(">/dev/null 2>&1 || true"));
    }
}
