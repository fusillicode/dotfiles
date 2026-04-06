use std::path::PathBuf;

use rootcause::option_ext::OptionExt as _;
use rootcause::prelude::ResultExt as _;
use serde::Deserialize;

use crate::agent::Agent;
use crate::agent::session::Session;

pub fn parse(content: &str, session_name: &str) -> rootcause::Result<Session> {
    let mut session = None;
    let mut first_user_message = None;

    for (line_idx, line) in content.lines().enumerate() {
        let record = serde_json::from_str::<CodexLine>(line)
            .context("failed to parse Codex session json line".to_owned())
            .attach(format!("line_number={}", line_idx.saturating_add(1)))
            .attach(format!("line={line}"))?;
        if first_user_message.is_none() {
            first_user_message = record.first_user_message();
        }

        let Some(payload) = record.into_session_meta() else {
            continue;
        };
        let created_at = chrono::DateTime::parse_from_rfc3339(&payload.timestamp)
            .ok()
            .map(|datetime| datetime.to_utc())
            .context("Codex timestamp is not RFC3339".to_owned())
            .attach(format!("line_number={}", line_idx.saturating_add(1)))
            .attach(format!("session_id={}", payload.id))
            .attach(format!("timestamp={}", payload.timestamp))?;

        session = Some(Session::new(
            Agent::Codex,
            payload.id,
            PathBuf::from(payload.cwd),
            first_user_message.clone(),
            created_at,
        ));
    }

    let mut session = session
        .context("no Codex session_meta record found".to_owned())
        .attach(format!("session_name={session_name}"))?;
    session.name = first_user_message.as_deref().unwrap_or(session_name).to_string();
    Ok(session)
}

#[derive(Debug, Deserialize)]
struct CodexSessionMetaPayload {
    id: String,
    cwd: String,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CodexLine {
    SessionMeta {
        #[serde(rename = "type")]
        _kind: CodexSessionMetaKind,
        payload: CodexSessionMetaPayload,
    },
    EventMsg {
        #[serde(rename = "type")]
        _kind: CodexEventMsgKind,
        payload: CodexEventPayload,
    },
    Other(serde::de::IgnoredAny),
}

#[derive(Debug, Deserialize)]
enum CodexSessionMetaKind {
    #[serde(rename = "session_meta")]
    SessionMeta,
}

#[derive(Debug, Deserialize)]
enum CodexEventMsgKind {
    #[serde(rename = "event_msg")]
    EventMsg,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CodexEventPayload {
    #[serde(rename = "user_message")]
    UserMessage { message: String },
    #[serde(other)]
    Other,
}

impl CodexLine {
    fn first_user_message(&self) -> Option<String> {
        match self {
            Self::EventMsg {
                payload: CodexEventPayload::UserMessage { message },
                ..
            } => Some(message.clone()),
            Self::SessionMeta { .. } | Self::EventMsg { .. } | Self::Other(_) => None,
        }
    }

    fn into_session_meta(self) -> Option<CodexSessionMetaPayload> {
        match self {
            Self::SessionMeta { payload, .. } => Some(payload),
            Self::EventMsg { .. } | Self::Other(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn parses_codex_session_from_session_meta() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let content = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"{}\",\"name\":\"Dotfiles\"}}}}\n",
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
    fn parses_codex_first_user_message_preview() {
        let content = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"why can't I jump with rust-analyzer to these types?\"}}\n"
        );
        assert2::assert!(let Ok(session) = parse(content, "fallback-name"));
        pretty_assertions::assert_eq!(session.name, "why can't I jump with rust-analyzer to these types?");
    }

    #[test]
    fn parses_codex_lines_with_unrelated_event_payloads() {
        let content = concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total\":1}}}\n",
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"assistant_message\",\"message\":\"hello\"}}\n"
        );
        assert2::assert!(let Ok(session) = parse(content, "fallback-name"));
        pretty_assertions::assert_eq!(session.id, "019d09f0-0d96-7e23-94cd-1f6aad7cdc09");
    }
}
