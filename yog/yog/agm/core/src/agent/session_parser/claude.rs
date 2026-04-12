use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;
use rootcause::option_ext::OptionExt as _;
use rootcause::prelude::ResultExt as _;
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::agent::Agent;
use crate::agent::session::Session;

pub fn parse(content: &str) -> rootcause::Result<Session> {
    let mut session = None;
    let mut first_user_message = None;

    for (line_idx, line) in content.lines().enumerate() {
        let line = serde_json::from_str::<ClaudeSessionLine>(line)
            .context("failed to parse Claude session json line".to_owned())
            .attach(format!("line_number={}", line_idx.saturating_add(1)))
            .attach(format!("line={line}"))?;

        if first_user_message.is_none() {
            first_user_message = line.first_user_message();
        }

        if session.is_none() {
            let Some(meta) = line.into_session_meta() else {
                continue;
            };
            let created_at = meta.timestamp;

            session = Some(Session::new(
                Agent::Claude,
                meta.session_id,
                PathBuf::from(meta.cwd),
                first_user_message.clone(),
                created_at,
            ));
        }

        if session.is_some() && first_user_message.is_some() {
            break;
        }
    }

    let mut session = session.context("no Claude session record found".to_owned())?;
    if let Some(first_user_message) = first_user_message {
        session.name = first_user_message;
    }

    for line in content.lines().rev() {
        let line = serde_json::from_str::<ClaudeSessionLine>(line)
            .context("failed to parse Claude session json line".to_owned())
            .attach(format!("line={line}"))?;
        let Some(timestamp) = line.timestamp() else {
            continue;
        };
        session.updated_at = timestamp;
        return Ok(session);
    }

    session.updated_at = session.created_at;
    Ok(session)
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClaudeSessionLine {
    #[serde(rename = "user")]
    User(ClaudeUserLine),
    #[serde(rename = "assistant")]
    #[serde(alias = "progress")]
    #[serde(alias = "system")]
    #[serde(alias = "attachment")]
    Metadata(ClaudeAgentLine),
    #[serde(rename = "queue-operation")]
    TimestampOnly(ClaudeTimestampedLine),
    #[serde(other)]
    Other,
}

impl ClaudeSessionLine {
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
            Self::User(line) => line.message.content.extract_text(),
            Self::Metadata(_) | Self::TimestampOnly(_) | Self::Other => None,
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
struct ClaudeAgentLine {
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

impl ClaudeAgentLine {
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
    content: ClaudeUserContent,
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ClaudeUserContent {
    Text(ClaudeUserText),
    Parts(Vec<ClaudeUserContentPart>),
}

impl ClaudeUserContent {
    fn extract_text(&self) -> Option<String> {
        match self {
            Self::Text(text) => text.preview(),
            Self::Parts(items) => items.iter().find_map(|item| {
                let ClaudeUserContentPart::Text { text } = item else {
                    return None;
                };
                text.preview()
            }),
        }
    }
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClaudeUserContentPart {
    Text {
        text: ClaudeUserText,
    },
    ToolResult {
        #[serde(rename = "content")]
        _content: IgnoredAny,
    },
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClaudeUserText {
    Plain(String),
    Cmd(ClaudeCmdInvocation),
}

impl ClaudeUserText {
    fn preview(&self) -> Option<String> {
        match self {
            Self::Plain(text) => Some(text.clone()),
            Self::Cmd(command) => command.preview(),
        }
    }
}

impl<'de> Deserialize<'de> for ClaudeUserText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Ok(self::ClaudeCmdInvocation::parse(&text)
            .map(Self::Cmd)
            .unwrap_or(Self::Plain(text)))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClaudeCommandTag {
    Name,
    Args,
}

impl ClaudeCommandTag {
    const fn open(self) -> &'static str {
        match self {
            Self::Name => "<command-name>",
            Self::Args => "<command-args>",
        }
    }

    const fn close(self) -> &'static str {
        match self {
            Self::Name => "</command-name>",
            Self::Args => "</command-args>",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClaudeCmdInvocation {
    name: String,
    args: Option<String>,
}

impl ClaudeCmdInvocation {
    fn parse(text: &str) -> Option<Self> {
        fn extract_tag(text: &str, tag: ClaudeCommandTag) -> Option<&str> {
            let start = text.find(tag.open())?.saturating_add(tag.open().len());
            let end = text[start..].find(tag.close())?.saturating_add(start);
            Some(&text[start..end])
        }

        let name = extract_tag(text, ClaudeCommandTag::Name)
            .map(str::trim)
            .filter(|name| !name.is_empty())?
            .to_owned();
        let args = extract_tag(text, ClaudeCommandTag::Args)
            .map(str::trim)
            .filter(|args| !args.is_empty())
            .map(str::to_owned);

        Some(Self { name, args })
    }

    fn preview(&self) -> Option<String> {
        let mut preview = self.name.clone();
        if let Some(command_args) = self.args.as_deref().map(str::trim).filter(|args| !args.is_empty()) {
            preview.push(' ');
            preview.push_str(command_args);
        }
        Some(preview)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_parse_claude_session_from_jsonl_lines_sets_workspace_and_id() {
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
    fn test_parse_claude_session_with_invalid_scanned_line_returns_error() {
        let content = "{\"type\":\"progress\",\"timestamp\":\"not-a-date\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n";

        assert2::assert!(let Err(err) = parse(content));
        assert!(err.to_string().contains("failed to parse Claude session json line"));
    }

    #[test]
    fn test_parse_claude_session_without_metadata_returns_error() {
        let content = "{\"type\":\"last-prompt\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\",\"lastPrompt\":\"hello\"}\n";
        assert2::assert!(let Err(err) = parse(content));
        assert!(err.to_string().contains("no Claude session record found"));
    }

    #[test]
    fn test_parse_claude_session_with_first_user_message_sets_name_and_updated_at() {
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

    #[test]
    fn test_parse_claude_session_with_command_wrapper_sets_command_preview() {
        let content = concat!(
            "{\"type\":\"progress\",\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"<command-message>privoly-admin</command-message>\\n<command-name>/privoly-admin</command-name>\\n<command-args>install</command-args>\"},\"timestamp\":\"2026-03-26T16:52:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n"
        );

        assert2::assert!(let Ok(session) = parse(content));
        pretty_assertions::assert_eq!(session.name, "/privoly-admin install");
    }

    #[test]
    fn test_extract_text_with_tool_result_then_text_returns_first_text_part() {
        let value = serde_json::from_str::<ClaudeUserContent>(
            r#"[{"type":"tool_result","content":"ignored"},{"type":"text","text":"later text"}]"#,
        )
        .unwrap();

        pretty_assertions::assert_eq!(value.extract_text(), Some("later text".to_owned()));
    }

    #[test]
    fn test_deserialize_claude_user_content_part_text_with_command_wrapper_models_command() {
        let value = serde_json::from_str::<ClaudeUserContent>(
            r#"[{"type":"text","text":"<command-message>privoly-admin</command-message>\n<command-name>/privoly-admin</command-name>\n<command-args>install</command-args>"}]"#,
        )
        .unwrap();

        pretty_assertions::assert_eq!(
            value,
            ClaudeUserContent::Parts(vec![ClaudeUserContentPart::Text {
                text: ClaudeUserText::Cmd(ClaudeCmdInvocation {
                    name: "/privoly-admin".to_owned(),
                    args: Some("install".to_owned()),
                }),
            }])
        );
    }

    #[test]
    fn test_parse_claude_session_skips_middle_lines_after_top_loop_stops() {
        let content = concat!(
            "{\"type\":\"progress\",\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"first user msg\"},\"timestamp\":\"2026-03-26T16:52:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"broken\"\n",
            "{\"type\":\"system\",\"timestamp\":\"2026-03-26T16:53:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"last-prompt\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\",\"lastPrompt\":\"hello\"}\n"
        );

        assert2::assert!(let Ok(session) = parse(content));
        pretty_assertions::assert_eq!(session.name, "first user msg");
        pretty_assertions::assert_eq!(
            session.updated_at,
            chrono::DateTime::parse_from_rfc3339("2026-03-26T16:53:02.119Z")
                .unwrap()
                .to_utc()
        );
    }
}
