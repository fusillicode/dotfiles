use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;
use rootcause::option_ext::OptionExt;
use rootcause::prelude::ResultExt;
use serde::Deserialize;
use serde::de::IgnoredAny;

use crate::agent::Agent;
use crate::agent::session::SearchTextBuilder;
use crate::agent::session::Session;

/// Parse one Claude JSONL session file.
///
/// # Errors
/// Returns an error when the JSONL cannot be parsed or required session metadata is missing.
pub fn parse(content: &str) -> rootcause::Result<ClaudeSession> {
    let mut session_id = None;
    let mut workspace_dir = None;
    let mut created_at = None;
    let mut updated_at = None;
    let mut first_user_message = None;
    let mut search_text = SearchTextBuilder::default();

    for (line_idx, line) in content.lines().enumerate() {
        let line = serde_json::from_str::<ClaudeSessionLine>(line)
            .context("failed to parse Claude session json line".to_owned())
            .attach(format!("line_number={}", line_idx.saturating_add(1)))
            .attach(format!("line={line}"))?;

        if let Some(timestamp) = line.timestamp() {
            updated_at = Some(timestamp);
        }

        if let Some(meta) = line.session_meta() {
            session_id.get_or_insert_with(|| meta.session_id.to_owned());
            workspace_dir.get_or_insert_with(|| PathBuf::from(meta.cwd));
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

    let session_id = session_id.context("no Claude session record found".to_owned())?;
    let workspace_dir = workspace_dir.context("no Claude session record found".to_owned())?;
    let created_at = created_at.context("no Claude session record found".to_owned())?;

    let name = first_user_message.unwrap_or_else(|| {
        workspace_dir
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map_or_else(|| session_id.clone(), str::to_owned)
    });
    let search_text = search_text.build(&name);

    Ok(ClaudeSession {
        id: session_id,
        name,
        search_text,
        workspace: workspace_dir,
        created_at,
        updated_at: updated_at.unwrap_or(created_at),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeSession {
    pub id: String,
    pub name: String,
    pub search_text: String,
    pub workspace: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClaudeSession {
    pub fn into_session(self, path: PathBuf) -> Session {
        let mut session = Session::new(Agent::Claude, self.id, self.workspace, path, None, self.created_at);
        session.name = self.name;
        session.search_text = self.search_text;
        session.updated_at = self.updated_at;
        session
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClaudeSessionLine {
    #[serde(rename = "user")]
    User(ClaudeUserLine),
    #[serde(rename = "assistant")]
    Assistant(ClaudeAssistantLine),
    #[serde(rename = "progress")]
    #[serde(alias = "system")]
    #[serde(alias = "attachment")]
    Metadata(ClaudeMetadataLine),
    #[serde(rename = "queue-operation")]
    TimestampOnly(ClaudeTimestampedLine),
    #[serde(other)]
    Other,
}

impl ClaudeSessionLine {
    const fn timestamp(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::User(line) => Some(line.timestamp),
            Self::Assistant(line) => Some(line.timestamp),
            Self::Metadata(line) => Some(line.timestamp),
            Self::TimestampOnly(line) => Some(line.timestamp),
            Self::Other => None,
        }
    }

    fn session_meta(&self) -> Option<ClaudeSessionMeta<'_>> {
        match self {
            Self::User(line) => Some(ClaudeSessionMeta::from(line)),
            Self::Assistant(line) => Some(ClaudeSessionMeta::from(line)),
            Self::Metadata(line) => Some(ClaudeSessionMeta::from(line)),
            Self::TimestampOnly(_) | Self::Other => None,
        }
    }

    fn user_search_text(&self) -> Option<String> {
        match self {
            Self::User(line) if !line.is_meta => line.message.content.search_text(),
            Self::User(_) | Self::Assistant(_) | Self::Metadata(_) | Self::TimestampOnly(_) | Self::Other => None,
        }
    }

    fn assistant_search_text(&self) -> Option<String> {
        match self {
            Self::Assistant(line) => line.message.search_text(),
            Self::User(_) | Self::Metadata(_) | Self::TimestampOnly(_) | Self::Other => None,
        }
    }
}

struct ClaudeSessionMeta<'a> {
    session_id: &'a str,
    cwd: &'a str,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUserLine {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    timestamp: DateTime<Utc>,
    #[serde(default, rename = "isMeta")]
    is_meta: bool,
    message: ClaudeUserMessage,
}

#[derive(Debug, Deserialize)]
struct ClaudeAssistantLine {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    timestamp: DateTime<Utc>,
    message: ClaudeAssistantMessage,
}

#[derive(Debug, Deserialize)]
struct ClaudeMetadataLine {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    timestamp: DateTime<Utc>,
}

impl<'a> From<&'a ClaudeUserLine> for ClaudeSessionMeta<'a> {
    fn from(value: &'a ClaudeUserLine) -> Self {
        Self {
            session_id: &value.session_id,
            cwd: &value.cwd,
            timestamp: value.timestamp,
        }
    }
}

impl<'a> From<&'a ClaudeAssistantLine> for ClaudeSessionMeta<'a> {
    fn from(value: &'a ClaudeAssistantLine) -> Self {
        Self {
            session_id: &value.session_id,
            cwd: &value.cwd,
            timestamp: value.timestamp,
        }
    }
}

impl<'a> From<&'a ClaudeMetadataLine> for ClaudeSessionMeta<'a> {
    fn from(value: &'a ClaudeMetadataLine) -> Self {
        Self {
            session_id: &value.session_id,
            cwd: &value.cwd,
            timestamp: value.timestamp,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ClaudeTimestampedLine {
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUserMessage {
    content: ClaudeUserContent,
}

#[derive(Debug, Deserialize)]
struct ClaudeAssistantMessage {
    #[serde(default)]
    content: Vec<ClaudeAssistantContentPart>,
}

impl ClaudeAssistantMessage {
    fn search_text(&self) -> Option<String> {
        let mut search_text = SearchTextBuilder::default();
        for snippet in self
            .content
            .iter()
            .filter_map(ClaudeAssistantContentPart::assistant_search_text)
        {
            search_text.push(snippet);
        }
        let search_text = search_text.build("");
        (!search_text.is_empty()).then_some(search_text)
    }
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ClaudeUserContent {
    Text(ClaudeUserText),
    Parts(Vec<ClaudeUserContentPart>),
}

impl ClaudeUserContent {
    fn search_text(&self) -> Option<String> {
        match self {
            Self::Text(text) => text.preview(),
            Self::Parts(items) => {
                let mut search_text = SearchTextBuilder::default();
                for snippet in items.iter().filter_map(|item| match item {
                    ClaudeUserContentPart::Text { text } => text.preview(),
                    ClaudeUserContentPart::ToolResult { .. } | ClaudeUserContentPart::Other => None,
                }) {
                    search_text.push(&snippet);
                }
                let search_text = search_text.build("");
                (!search_text.is_empty()).then_some(search_text)
            }
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

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClaudeAssistantContentPart {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

impl ClaudeAssistantContentPart {
    fn assistant_search_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            Self::Other => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClaudeUserText {
    Plain(String),
    Cmd(ClaudeCmdInvocation),
}

impl ClaudeUserText {
    fn preview(&self) -> Option<String> {
        match self {
            Self::Plain(text)
                if matches!(
                    text.trim_start(),
                    text if text.starts_with("<local-command-caveat>")
                        || text.starts_with("<local-command-stdout>")
                ) =>
            {
                None
            }
            Self::Plain(text) => Some(text.clone()),
            Self::Cmd(command) => Some(command.preview()),
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
            let tail = text.get(start..)?;
            let end = tail.find(tag.close())?.saturating_add(start);
            text.get(start..end)
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

    fn preview(&self) -> String {
        let mut preview = self.name.clone();
        if let Some(command_args) = self.args.as_deref().map(str::trim).filter(|args| !args.is_empty()) {
            preview.push(' ');
            preview.push_str(command_args);
        }
        preview
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

        assert2::assert!(let Ok(claude_session) = parse(&content));
        let session = claude_session.into_session(workspace.join("session.jsonl"));
        pretty_assertions::assert_eq!(session.agent, Agent::Claude);
        pretty_assertions::assert_eq!(session.workspace, workspace);
        pretty_assertions::assert_eq!(session.id, "8649a076-3ead-4d5a-9840-3200f0e1aae5");
        pretty_assertions::assert_eq!(session.name, "workspace");
        pretty_assertions::assert_eq!(session.search_text, "workspace");
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
    fn test_parse_claude_session_indexes_user_and_assistant_text() {
        let content = concat!(
            "{\"type\":\"progress\",\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"this is a very long first user message\"},\"timestamp\":\"2026-03-26T16:52:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"hidden\"},{\"type\":\"text\",\"text\":\"assistant answer\"},{\"type\":\"tool_use\",\"name\":\"Read\"}]},\"timestamp\":\"2026-03-26T16:53:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n"
        );

        assert2::assert!(let Ok(claude_session) = parse(content));
        let session = claude_session.into_session(PathBuf::from("session.jsonl"));
        pretty_assertions::assert_eq!(session.name, "this is a very long first user message");
        pretty_assertions::assert_eq!(
            session.search_text,
            "this is a very long first user message assistant answer"
        );
        pretty_assertions::assert_eq!(
            session.updated_at,
            chrono::DateTime::parse_from_rfc3339("2026-03-26T16:53:02.119Z")
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

        assert2::assert!(let Ok(claude_session) = parse(content));
        let session = claude_session.into_session(PathBuf::from("session.jsonl"));
        pretty_assertions::assert_eq!(session.name, "/privoly-admin install");
        pretty_assertions::assert_eq!(session.search_text, "/privoly-admin install");
    }

    #[test]
    fn test_parse_claude_session_skips_meta_and_tool_result_only_user_rows() {
        let content = concat!(
            "{\"type\":\"progress\",\"timestamp\":\"2026-03-26T16:51:01.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"role\":\"user\",\"content\":\"<local-command-caveat>ignore me</local-command-caveat>\"},\"timestamp\":\"2026-03-26T16:52:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"content\":\"ignored\"}]},\"timestamp\":\"2026-03-26T16:53:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n",
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"real prompt\"},\"timestamp\":\"2026-03-26T16:54:02.119Z\",\"cwd\":\"/tmp/workspace\",\"sessionId\":\"8649a076-3ead-4d5a-9840-3200f0e1aae5\"}\n"
        );

        assert2::assert!(let Ok(claude_session) = parse(content));
        let session = claude_session.into_session(PathBuf::from("session.jsonl"));
        pretty_assertions::assert_eq!(session.name, "real prompt");
        pretty_assertions::assert_eq!(session.search_text, "real prompt");
    }

    #[test]
    fn test_user_content_search_text_with_tool_result_then_text_returns_text() {
        let value = serde_json::from_str::<ClaudeUserContent>(
            r#"[{"type":"tool_result","content":"ignored"},{"type":"text","text":"later text"}]"#,
        )
        .unwrap();

        pretty_assertions::assert_eq!(value.search_text(), Some("later text".to_owned()));
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
}
