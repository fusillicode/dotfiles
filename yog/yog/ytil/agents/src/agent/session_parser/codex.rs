use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;
use rootcause::option_ext::OptionExt as _;
use rootcause::prelude::ResultExt as _;
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::agent::Agent;
use crate::agent::session::SearchTextBuilder;
use crate::agent::session::Session;

/// Parse one Codex JSONL session file.
///
/// # Errors
/// Returns an error when the JSONL cannot be parsed or required session metadata is missing.
pub fn parse(content: &str, session_name: &str) -> rootcause::Result<Session> {
    let mut session_id = None;
    let mut workspace_dir = None;
    let mut created_at = None;
    let mut updated_at = None;
    let mut first_user_message = None;
    let mut search_text = SearchTextBuilder::default();

    for (line_idx, line) in content.lines().enumerate() {
        let line = serde_json::from_str::<CodexLine>(line)
            .context("failed to parse Codex session json line".to_owned())
            .attach(format!("line_number={}", line_idx.saturating_add(1)))
            .attach(format!("line={line}"))?;

        if let Some(timestamp) = line.timestamp() {
            updated_at = Some(timestamp);
        }

        if let Some(meta) = line.session_meta() {
            session_id.get_or_insert_with(|| meta.id.clone());
            workspace_dir.get_or_insert_with(|| PathBuf::from(&meta.cwd));
            created_at.get_or_insert(meta.timestamp);
        }

        if let Some(user_message) = line.user_search_text() {
            if first_user_message.is_none() {
                first_user_message = Some(user_message.clone());
            }
            search_text.push(&user_message);
        }
        if let Some(assistant_message) = line.assistant_search_text() {
            search_text.push(&assistant_message);
        }
    }

    let session_id = session_id
        .context("no Codex session_meta record found".to_owned())
        .attach(format!("session_name={session_name}"))?;
    let workspace_dir = workspace_dir
        .context("no Codex session_meta record found".to_owned())
        .attach(format!("session_name={session_name}"))?;
    let created_at = created_at
        .context("no Codex session_meta record found".to_owned())
        .attach(format!("session_name={session_name}"))?;

    let mut session = Session::new(
        Agent::Codex,
        session_id,
        workspace_dir,
        first_user_message.clone().or_else(|| Some(session_name.to_owned())),
        created_at,
    );
    session.name = first_user_message.unwrap_or_else(|| session_name.to_owned());
    session.search_text = search_text.build(&session.name);
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
    ResponseItem(CodexResponseItemLine),
    #[serde(alias = "turn_context")]
    #[serde(alias = "compacted")]
    Timestamped(CodexTimestampedLine),
    #[serde(other)]
    Other,
}

impl CodexLine {
    const fn timestamp(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::SessionMeta(line) => Some(line.timestamp),
            Self::EventMsg(line) => Some(line.timestamp),
            Self::ResponseItem(line) => Some(line.timestamp),
            Self::Timestamped(line) => Some(line.timestamp),
            Self::Other => None,
        }
    }

    const fn session_meta(&self) -> Option<&CodexSessionMetaPayload> {
        match self {
            Self::SessionMeta(line) => Some(&line.payload),
            Self::EventMsg(_) | Self::ResponseItem(_) | Self::Timestamped(_) | Self::Other => None,
        }
    }

    fn user_search_text(&self) -> Option<String> {
        match self {
            Self::EventMsg(line) => line.user_search_text(),
            Self::SessionMeta(_) | Self::ResponseItem(_) | Self::Timestamped(_) | Self::Other => None,
        }
    }

    fn assistant_search_text(&self) -> Option<String> {
        match self {
            Self::ResponseItem(line) => line.assistant_search_text(),
            Self::SessionMeta(_) | Self::EventMsg(_) | Self::Timestamped(_) | Self::Other => None,
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
    fn user_search_text(&self) -> Option<String> {
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
struct CodexResponseItemLine {
    timestamp: DateTime<Utc>,
    payload: CodexResponseItemPayload,
}

impl CodexResponseItemLine {
    fn assistant_search_text(&self) -> Option<String> {
        match &self.payload {
            CodexResponseItemPayload::Message { role, content } if role == "assistant" => {
                let mut search_text = SearchTextBuilder::default();
                for snippet in content
                    .iter()
                    .filter_map(CodexMessageContentPart::assistant_search_text)
                {
                    search_text.push(snippet);
                }
                let message = search_text.build("");
                (!message.is_empty()).then_some(message)
            }
            CodexResponseItemPayload::Message { .. }
            | CodexResponseItemPayload::Reasoning
            | CodexResponseItemPayload::Other => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CodexResponseItemPayload {
    #[serde(rename = "message")]
    Message {
        role: String,
        #[serde(default)]
        content: Vec<CodexMessageContentPart>,
    },
    #[serde(rename = "reasoning")]
    Reasoning,
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum CodexMessageContentPart {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "input_text")]
    InputText {
        #[serde(rename = "text")]
        _text: IgnoredAny,
    },
    #[serde(other)]
    Other,
}

impl CodexMessageContentPart {
    fn assistant_search_text(&self) -> Option<&str> {
        match self {
            Self::OutputText { text } => Some(text),
            Self::InputText { .. } | Self::Other => None,
        }
    }
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
    fn test_parse_codex_session_from_session_meta_uses_session_name_fallback() {
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
        pretty_assertions::assert_eq!(
            session.search_text,
            "rollout-2026-03-20T07-30-20-019d09f0-0d96-7e23-94cd-1f6aad7cdc09"
        );
        pretty_assertions::assert_eq!(session.workspace, workspace);
    }

    #[test]
    fn test_parse_codex_session_indexes_user_and_assistant_text_and_updated_at() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:31:20.312Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"why can't I jump with rust-analyzer to these types?\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:32:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Because that symbol is re-exported.\"},{\"type\":\"input_text\",\"text\":\"ignored\"}]}}\n",
            "{\"timestamp\":\"2026-03-20T06:33:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"reasoning\",\"text\":\"hidden\"}}\n"
        );

        assert2::assert!(let Ok(session) = parse(content, "fallback-name"));
        pretty_assertions::assert_eq!(session.name, "why can't I jump with rust-analyzer to these types?");
        pretty_assertions::assert_eq!(
            session.search_text,
            "why can't I jump with rust-analyzer to these types? Because that symbol is re-exported."
        );
        pretty_assertions::assert_eq!(
            session.updated_at,
            chrono::DateTime::parse_from_rfc3339("2026-03-20T06:33:20.312Z")
                .unwrap()
                .to_utc()
        );
    }

    #[test]
    fn test_parse_codex_session_ignores_non_assistant_response_text() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:31:20.312Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"first user msg\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:32:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"output_text\",\"text\":\"should not index\"}]}}\n"
        );

        assert2::assert!(let Ok(session) = parse(content, "fallback-name"));
        pretty_assertions::assert_eq!(session.search_text, "first user msg");
    }

    #[test]
    fn test_parse_codex_session_with_invalid_scanned_line_returns_error() {
        let content = "{\"timestamp\":\"not-a-date\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n";

        assert2::assert!(let Err(err) = parse(content, "fallback-name"));
        assert!(err.to_string().contains("failed to parse Codex session json line"));
    }
}
