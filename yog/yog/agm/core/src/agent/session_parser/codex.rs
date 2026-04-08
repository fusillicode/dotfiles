use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;
use rootcause::option_ext::OptionExt as _;
use rootcause::prelude::ResultExt as _;
use serde::Deserialize;

use crate::agent::Agent;
use crate::agent::session::Session;

pub fn parse(content: &str, session_name: &str) -> rootcause::Result<Session> {
    let mut session = None;
    let mut first_user_message = None;
    let mut updated_at = None;

    for (line_idx, line) in content.lines().enumerate() {
        let line = serde_json::from_str::<CodexLine>(line)
            .context("failed to parse Codex session json line".to_owned())
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
        let created_at = meta.payload.timestamp;

        session.get_or_insert_with(|| {
            Session::new(
                Agent::Codex,
                meta.payload.id,
                PathBuf::from(meta.payload.cwd),
                first_user_message.clone(),
                created_at,
            )
        });
    }

    let mut session = session
        .context("no Codex session_meta record found".to_owned())
        .attach(format!("session_name={session_name}"))?;
    session.name = first_user_message.as_deref().unwrap_or(session_name).to_string();
    session.updated_at = updated_at.unwrap_or(session.created_at);
    Ok(session)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CodexLine {
    #[serde(rename = "session_meta")]
    SessionMeta(CodexSessionMetaLine),
    #[serde(rename = "event_msg")]
    EventMsg(CodexEventMsgLine),
    #[serde(rename = "response_item")]
    #[serde(alias = "turn_context")]
    #[serde(alias = "compacted")]
    Timestamped(CodexTimestampedLine),
    #[serde(other)]
    Other,
}

impl CodexLine {
    fn timestamp(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::SessionMeta(line) => Some(line.timestamp),
            Self::EventMsg(line) => Some(line.timestamp),
            Self::Timestamped(line) => Some(line.timestamp),
            Self::Other => None,
        }
    }

    fn first_user_message(&self) -> Option<String> {
        match self {
            Self::EventMsg(line) => line.first_user_message(),
            Self::SessionMeta(_) | Self::Timestamped(_) | Self::Other => None,
        }
    }

    fn into_session_meta(self) -> Option<CodexSessionMetaLine> {
        match self {
            Self::SessionMeta(line) => Some(line),
            Self::EventMsg(_) | Self::Timestamped(_) | Self::Other => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CodexSessionMetaLine {
    #[serde(rename = "timestamp")]
    timestamp: DateTime<Utc>,
    payload: CodexSessionMetaPayload,
}

#[derive(Debug, Deserialize)]
struct CodexSessionMetaPayload {
    id: String,
    cwd: String,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct CodexEventMsgLine {
    #[serde(rename = "timestamp")]
    timestamp: DateTime<Utc>,
    payload: CodexEventPayload,
}

impl CodexEventMsgLine {
    fn first_user_message(&self) -> Option<String> {
        match &self.payload {
            CodexEventPayload::UserMessage { message } => Some(message.clone()),
            CodexEventPayload::Other => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CodexEventPayload {
    #[serde(rename = "user_message")]
    UserMessage { message: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct CodexTimestampedLine {
    timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_parses_codex_session_from_session_meta() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let content = format!(
            "{{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"{}\",\"name\":\"Dotfiles\"}}}}\n",
            workspace.display()
        );

        assert2::assert!(let Ok(session) = parse(
            &content,
            "rollout-2026-03-20T07-30-20-019d09f0-0d96-7e23-94cd-1f6aad7cdc09",
        ));
        pretty_assertions::assert_eq!(session.agent, Agent::Codex);
        pretty_assertions::assert_eq!(
            session.name,
            "rollout-2026-03-20T07-30-20-019d09f0-0d96-7e23-94cd-1f6aad7cdc09"
        );
        pretty_assertions::assert_eq!(session.workspace, workspace);
    }

    #[test]
    fn test_parses_codex_first_user_message_preview() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:31:20.312Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"why can't I jump with rust-analyzer to these types?\"}}\n"
        );
        assert2::assert!(let Ok(session) = parse(content, "fallback-name"));
        pretty_assertions::assert_eq!(session.name, "why can't I jump with rust-analyzer to these types?");
        pretty_assertions::assert_eq!(
            session.updated_at,
            chrono::DateTime::parse_from_rfc3339("2026-03-20T06:31:20.312Z")
                .unwrap()
                .to_utc()
        );
    }

    #[test]
    fn test_parses_codex_lines_with_unrelated_event_payloads() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:29:20.312Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total\":1}}}\n",
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:32:20.312Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"assistant_message\",\"message\":\"hello\"}}\n"
        );
        assert2::assert!(let Ok(session) = parse(content, "fallback-name"));
        pretty_assertions::assert_eq!(session.id, "019d09f0-0d96-7e23-94cd-1f6aad7cdc09");
        pretty_assertions::assert_eq!(
            session.updated_at,
            chrono::DateTime::parse_from_rfc3339("2026-03-20T06:32:20.312Z")
                .unwrap()
                .to_utc()
        );
    }
}
