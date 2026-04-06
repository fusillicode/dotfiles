use std::path::PathBuf;

use rootcause::option_ext::OptionExt as _;
use rootcause::prelude::ResultExt as _;
use serde::Deserialize;

use crate::agent::Agent;
use crate::agent::session::Session;

pub fn parse(content: &str) -> rootcause::Result<Session> {
    let mut session = None;
    let mut first_user_message = None;

    for (line_idx, line) in content.lines().enumerate() {
        let record = serde_json::from_str::<ClaudeLine>(line)
            .context("failed to parse Claude session json line".to_owned())
            .attach(format!("line_number={}", line_idx.saturating_add(1)))
            .attach(format!("line={line}"))?;

        if first_user_message.is_none() {
            first_user_message = record.first_user_message();
        }

        let Some(meta) = record.into_session_meta() else {
            continue;
        };
        let created_at = chrono::DateTime::parse_from_rfc3339(&meta.timestamp)
            .ok()
            .map(|datetime| datetime.to_utc())
            .context("Claude timestamp is not RFC3339".to_owned())
            .attach(format!("line_number={}", line_idx.saturating_add(1)))
            .attach(format!("session_id={}", meta.session_id))
            .attach(format!("timestamp={}", meta.timestamp))?;

        session = Some(Session::new(
            Agent::Claude,
            meta.session_id,
            PathBuf::from(meta.cwd),
            first_user_message.clone(),
            created_at,
        ));
        if first_user_message.is_some() {
            break;
        }
    }

    let mut session = session.context("no Claude session record found".to_owned())?;
    if let Some(first_user_message) = first_user_message {
        session.name = first_user_message;
    }
    Ok(session)
}

#[derive(Debug, Deserialize)]
struct ClaudeSessionMeta {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    role: Option<String>,
    content: Option<ClaudeContent>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum ClaudeContent {
    Text(String),
    Parts(Vec<ClaudeContent>),
    Rich {
        text: Option<String>,
        content: Option<Box<ClaudeContent>>,
    },
}

impl ClaudeContent {
    fn into_text(self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text),
            Self::Parts(parts) => parts.into_iter().find_map(Self::into_text),
            Self::Rich { text, content } => text.or_else(|| content.and_then(|value| value.into_text())),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    message: Option<ClaudeMessage>,
    #[serde(flatten)]
    session_meta: Option<ClaudeSessionMeta>,
}

impl ClaudeLine {
    fn first_user_message(&self) -> Option<String> {
        (self.kind.as_deref() == Some("user"))
            .then_some(self.message.as_ref())
            .flatten()
            .filter(|message| message.role.as_deref() == Some("user"))
            .and_then(|message| message.content.as_ref())
            .cloned()
            .and_then(ClaudeContent::into_text)
    }

    fn into_session_meta(self) -> Option<ClaudeSessionMeta> {
        self.session_meta
    }
}

#[cfg(test)]
mod tests {

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parses_claude_session_from_jsonl_lines() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let content = concat!(
            "{\"type\":\"file-history-snapshot\"}\n",
            "{\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"__CWD__\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"slug\":\"cosmic-moseying-feigenbaum\"}\n"
        )
        .replace("__CWD__", &workspace.display().to_string());

        assert2::assert!(let Ok(session) = parse(&content));
        pretty_assertions::assert_eq!(session.agent, Agent::Claude);
        pretty_assertions::assert_eq!(session.workspace, workspace);
        pretty_assertions::assert_eq!(session.id, "8649a076-3ead-4d5a-9840-3200f0e1aae5");
        pretty_assertions::assert_eq!(session.name, "workspace");
    }

    #[test]
    fn parsing_invalid_claude_timestamp_returns_error() {
        let content = "{\"timestamp\":\"not-a-date\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n";

        assert2::assert!(let Err(err) = parse(content));
        assert!(err.to_string().contains("Claude timestamp is not RFC3339"));
    }

    #[test]
    fn parsing_claude_line_without_cwd_is_skipped() {
        let content = "{\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n";
        assert2::assert!(let Err(err) = parse(content));
        assert!(err.to_string().contains("no Claude session record found"));
    }

    #[test]
    fn parses_claude_first_user_message_preview() {
        let content = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"this is a very long first user message\"},\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n";
        assert2::assert!(let Ok(session) = parse(content));
        pretty_assertions::assert_eq!(session.name, "this is a very long first user message");
    }
}
