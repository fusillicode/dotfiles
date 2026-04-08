use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;
use rootcause::option_ext::OptionExt as _;
use rootcause::prelude::ResultExt as _;
use serde::Deserialize;
use serde_json::Value;

use crate::agent::Agent;
use crate::agent::session::Session;

pub fn parse(content: &str) -> rootcause::Result<Session> {
    let mut session = None;
    let mut first_user_message = None;
    let mut updated_at = None;

    for (line_idx, line) in content.lines().enumerate() {
        let line = serde_json::from_str::<ClaudeLine>(line)
            .context("failed to parse Claude session json line".to_owned())
            .attach(format!("line_number={}", line_idx.saturating_add(1)))
            .attach(format!("line={line}"))?;

        if let Some(timestamp) = line.timestamp() {
            updated_at = Some(timestamp);
        }

        if first_user_message.is_none() {
            first_user_message = line.first_user_message();
        }

        let Some(meta) = line.into_session_meta() else {
            continue;
        };
        let created_at = meta.timestamp;

        session.get_or_insert_with(|| {
            Session::new(
                Agent::Claude,
                meta.session_id,
                PathBuf::from(meta.cwd),
                first_user_message.clone(),
                created_at,
            )
        });
    }

    let mut session = session.context("no Claude session record found".to_owned())?;
    if let Some(first_user_message) = first_user_message {
        session.name = first_user_message;
    }
    session.updated_at = updated_at.unwrap_or(session.created_at);
    Ok(session)
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClaudeLine {
    #[serde(rename = "user")]
    User(ClaudeUserLine),
    #[serde(rename = "assistant")]
    #[serde(alias = "progress")]
    #[serde(alias = "system")]
    #[serde(alias = "attachment")]
    Metadata(ClaudeSessionLine),
    #[serde(rename = "queue-operation")]
    TimestampOnly(ClaudeTimestampedLine),
    #[serde(other)]
    Other,
}

impl ClaudeLine {
    fn timestamp(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::User(line) => Some(line.timestamp),
            Self::Metadata(line) => Some(line.timestamp),
            Self::TimestampOnly(line) => Some(line.timestamp),
            Self::Other => None,
        }
    }

    fn first_user_message(&self) -> Option<String> {
        match self {
            Self::User(line) if line.message.role.as_deref() == Some("user") => extract_text(&line.message.content),
            Self::User(_) | Self::Metadata(_) | Self::TimestampOnly(_) | Self::Other => None,
        }
    }

    fn into_session_meta(self) -> Option<ClaudeSessionMeta> {
        match self {
            Self::User(line) => line.into_session_meta(),
            Self::Metadata(line) => line.into_session_meta(),
            Self::TimestampOnly(_) | Self::Other => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeSessionMeta {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUserLine {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    timestamp: DateTime<Utc>,
    message: ClaudeMessage,
}

#[derive(Debug, Deserialize)]
struct ClaudeSessionLine {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    timestamp: DateTime<Utc>,
}

impl ClaudeUserLine {
    fn into_session_meta(self) -> Option<ClaudeSessionMeta> {
        Some(ClaudeSessionMeta {
            session_id: self.session_id,
            cwd: self.cwd,
            timestamp: self.timestamp,
        })
    }
}

impl ClaudeSessionLine {
    fn into_session_meta(self) -> Option<ClaudeSessionMeta> {
        Some(ClaudeSessionMeta {
            session_id: self.session_id,
            cwd: self.cwd,
            timestamp: self.timestamp,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ClaudeTimestampedLine {
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    role: Option<String>,
    content: Value,
}

fn extract_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => items.iter().find_map(extract_text),
        Value::Object(map) => map
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| map.get("content").and_then(extract_text)),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_parses_claude_session_from_jsonl_lines() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let content = concat!(
            "{\"type\":\"file-history-snapshot\",\"messageId\":\"m1\",\"snapshot\":{},\"isSnapshotUpdate\":false}\n",
            "{\"type\":\"progress\",\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"__CWD__\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"last-prompt\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\",\"lastPrompt\":\"hello\"}\n"
        )
        .replace("__CWD__", &workspace.display().to_string());

        assert2::assert!(let Ok(session) = parse(&content));
        pretty_assertions::assert_eq!(session.agent, Agent::Claude);
        pretty_assertions::assert_eq!(session.workspace, workspace);
        pretty_assertions::assert_eq!(session.id, "8649a076-3ead-4d5a-9840-3200f0e1aae5");
        pretty_assertions::assert_eq!(session.name, "workspace");
    }

    #[test]
    fn test_parsing_invalid_claude_timestamp_returns_error() {
        let content = "{\"type\":\"progress\",\"timestamp\":\"not-a-date\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n";

        assert2::assert!(let Err(err) = parse(content));
        assert!(err.to_string().contains("failed to parse Claude session json line"));
    }

    #[test]
    fn test_parsing_claude_line_without_cwd_is_skipped() {
        let content = "{\"type\":\"last-prompt\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\",\"lastPrompt\":\"hello\"}\n";
        assert2::assert!(let Err(err) = parse(content));
        assert!(err.to_string().contains("no Claude session record found"));
    }

    #[test]
    fn test_parses_claude_first_user_message_preview() {
        let content = concat!(
            "{\"type\":\"progress\",\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"this is a very long first user message\"},\"timestamp\":\"2026-03-26T16:52:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n"
        );
        assert2::assert!(let Ok(session) = parse(content));
        pretty_assertions::assert_eq!(session.name, "this is a very long first user message");
        pretty_assertions::assert_eq!(
            session.updated_at,
            chrono::DateTime::parse_from_rfc3339("2026-03-26T16:52:02.119Z")
                .unwrap()
                .to_utc()
        );
    }
}
