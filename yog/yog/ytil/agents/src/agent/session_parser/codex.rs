use std::io::BufRead;
use std::path::PathBuf;

use jiff::Timestamp;
use rootcause::option_ext::OptionExt;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;

use crate::agent::Agent;
use crate::agent::session::SearchTextBuilder;
use crate::agent::session::Session;

const AGENTS_INSTRUCTIONS_PREFIX: &str = "# AGENTS.md instructions";
const ENVIRONMENT_CONTEXT_PREFIX: &str = "<environment_context>";

struct SearchTextSnippet {
    text: String,
    normalized: bool,
}

impl SearchTextSnippet {
    const fn raw(text: String) -> Self {
        Self {
            text,
            normalized: false,
        }
    }

    const fn normalized(text: String) -> Self {
        Self { text, normalized: true }
    }

    fn push_to(&self, search_text: &mut SearchTextBuilder) {
        if self.normalized {
            search_text.push_normalized(&self.text);
        } else {
            search_text.push(&self.text);
        }
    }
}

/// Parse one Codex JSONL session file.
///
/// # Errors
/// Returns an error when the JSONL cannot be parsed or required session metadata is missing.
pub fn parse(content: &str, session_name: &str) -> rootcause::Result<CodexSession> {
    let mut parser = CodexSessionParser::default();

    for (line_idx, line) in content.lines().enumerate() {
        parser.push_line(line, line_idx.saturating_add(1))?;
    }

    parser.finish(session_name)
}

/// Parse the metadata and first real user prompt from a Codex session reader.
///
/// Stops reading once the list preview is complete.
pub(crate) fn parse_preview(reader: impl BufRead, session_name: &str) -> rootcause::Result<CodexSession> {
    let mut parser = CodexSessionParser::default();
    for (line_idx, line) in reader.lines().enumerate() {
        let line = line
            .context("failed to read Codex session json line")
            .attach(format!("line_number={}", line_idx.saturating_add(1)))?;
        parser.push_line(&line, line_idx.saturating_add(1))?;
        if parser.is_subagent || parser.is_preview_complete() {
            break;
        }
    }

    parser.finish(session_name)
}

/// Read the first valid Codex session metadata record.
///
/// # Errors
/// Returns an error when metadata cannot be read before a valid `session_meta`
/// record is found. Later malformed JSONL records are intentionally ignored.
pub(crate) fn parse_metadata_for_deletion(
    reader: impl BufRead,
    session_name: &str,
) -> rootcause::Result<CodexSessionMetadata> {
    for (line_idx, line) in reader.lines().enumerate() {
        let line = line
            .context("failed to read Codex session json line")
            .attach(format!("line_number={}", line_idx.saturating_add(1)))?;
        let line = serde_json::from_str::<CodexLine>(&line)
            .context("failed to parse Codex session json line")
            .attach(format!("line_number={}", line_idx.saturating_add(1)))?;
        if let Some(meta) = line.session_meta() {
            return Ok(CodexSessionMetadata {
                id: meta.id.clone(),
                parent_thread_id: meta.parent_thread_id.clone(),
            });
        }
    }

    Err(report!("no Codex session_meta record found").attach(format!("session_name={session_name}")))
}

#[derive(Default)]
struct CodexSessionParser {
    session_id: Option<String>,
    workspace_dir: Option<PathBuf>,
    created_at: Option<Timestamp>,
    updated_at: Option<Timestamp>,
    first_user_message: Option<String>,
    is_subagent: bool,
    search_text: SearchTextBuilder,
}

impl CodexSessionParser {
    fn push_line(&mut self, line: &str, line_number: usize) -> rootcause::Result<()> {
        let line = serde_json::from_str::<CodexLine>(line)
            .context("failed to parse Codex session json line".to_owned())
            .attach(format!("line_number={line_number}"))
            .attach(format!("line={line}"))?;

        if let Some(timestamp) = line.timestamp() {
            self.updated_at = Some(timestamp);
        }

        if let Some(meta) = line.session_meta() {
            self.session_id.get_or_insert_with(|| meta.id.clone());
            self.workspace_dir.get_or_insert_with(|| PathBuf::from(&meta.cwd));
            self.created_at.get_or_insert(meta.timestamp);
            self.is_subagent |= meta.is_subagent();
        }

        if let Some(user_message) = line.user_search_text() {
            if self.first_user_message.is_none() {
                self.first_user_message = Some(user_message.text.clone());
            }
            user_message.push_to(&mut self.search_text);
        }
        if let Some(assistant_message) = line.assistant_search_text() {
            assistant_message.push_to(&mut self.search_text);
        }
        Ok(())
    }

    const fn is_preview_complete(&self) -> bool {
        self.session_id.is_some()
            && self.workspace_dir.is_some()
            && self.created_at.is_some()
            && self.first_user_message.is_some()
    }

    fn finish(self, session_name: &str) -> rootcause::Result<CodexSession> {
        let session_id = self
            .session_id
            .context("no Codex session_meta record found".to_owned())
            .attach(format!("session_name={session_name}"))?;
        let workspace_dir = self
            .workspace_dir
            .context("no Codex session_meta record found".to_owned())
            .attach(format!("session_name={session_name}"))?;
        let created_at = self
            .created_at
            .context("no Codex session_meta record found".to_owned())
            .attach(format!("session_name={session_name}"))?;

        let name = self.first_user_message.unwrap_or_else(|| session_name.to_owned());
        let search_text = self.search_text.build(&name);

        Ok(CodexSession {
            id: session_id,
            name,
            search_text,
            workspace: workspace_dir,
            created_at,
            updated_at: self.updated_at.unwrap_or(created_at),
            is_subagent: self.is_subagent,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexSession {
    pub id: String,
    pub name: String,
    pub search_text: String,
    pub workspace: PathBuf,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub is_subagent: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CodexSessionMetadata {
    pub(crate) id: String,
    pub(crate) parent_thread_id: Option<String>,
}

impl CodexSession {
    pub fn into_session(self, path: PathBuf) -> Session {
        let mut session = Session::new(Agent::Codex, self.id, self.workspace, path, None, self.created_at);
        session.name = self.name;
        session.search_text = self.search_text;
        session.updated_at = self.updated_at;
        session
    }
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
    const fn timestamp(&self) -> Option<Timestamp> {
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

    fn user_search_text(&self) -> Option<SearchTextSnippet> {
        match self {
            Self::EventMsg(line) => line.user_search_text().map(SearchTextSnippet::raw),
            Self::ResponseItem(line) => line.user_search_text(),
            Self::SessionMeta(_) | Self::Timestamped(_) | Self::Other => None,
        }
    }

    fn assistant_search_text(&self) -> Option<SearchTextSnippet> {
        match self {
            Self::ResponseItem(line) => line.assistant_search_text(),
            Self::SessionMeta(_) | Self::EventMsg(_) | Self::Timestamped(_) | Self::Other => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CodexSessionMetaLine {
    #[serde(rename = "timestamp")]
    timestamp: Timestamp,
    payload: CodexSessionMetaPayload,
}

#[derive(Debug, Deserialize)]
struct CodexSessionMetaPayload {
    id: String,
    parent_thread_id: Option<String>,
    cwd: String,
    timestamp: Timestamp,
    source: Option<serde_json::Value>,
}

impl CodexSessionMetaPayload {
    fn is_subagent(&self) -> bool {
        self.source
            .as_ref()
            .is_some_and(|source| source.get("subagent").is_some())
    }
}

#[derive(Debug, Deserialize)]
struct CodexEventMsgLine {
    #[serde(rename = "timestamp")]
    timestamp: Timestamp,
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
    timestamp: Timestamp,
    payload: CodexResponseItemPayload,
}

impl CodexResponseItemLine {
    fn user_search_text(&self) -> Option<SearchTextSnippet> {
        match &self.payload {
            CodexResponseItemPayload::Message { role, content }
                if role == "user" && !is_injected_agents_context(content) =>
            {
                search_text_from_content(content, CodexMessageContentPart::user_search_text)
                    .map(SearchTextSnippet::normalized)
            }
            CodexResponseItemPayload::Message { .. }
            | CodexResponseItemPayload::Reasoning
            | CodexResponseItemPayload::Other => None,
        }
    }

    fn assistant_search_text(&self) -> Option<SearchTextSnippet> {
        match &self.payload {
            CodexResponseItemPayload::Message { role, content } if role == "assistant" => {
                search_text_from_content(content, CodexMessageContentPart::assistant_search_text)
                    .map(SearchTextSnippet::normalized)
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
    InputText { text: serde_json::Value },
    #[serde(other)]
    Other,
}

impl CodexMessageContentPart {
    fn input_text(&self) -> Option<&str> {
        match self {
            Self::InputText { text } => text.as_str(),
            Self::OutputText { .. } | Self::Other => None,
        }
    }

    fn assistant_search_text(&self) -> Option<&str> {
        match self {
            Self::OutputText { text } => Some(text),
            Self::InputText { .. } | Self::Other => None,
        }
    }

    fn user_search_text(&self) -> Option<&str> {
        self.input_text()
    }
}

fn is_injected_agents_context(content: &[CodexMessageContentPart]) -> bool {
    let mut input_texts = content.iter().filter_map(CodexMessageContentPart::input_text);
    input_texts
        .next()
        .is_some_and(|text| text.starts_with(AGENTS_INSTRUCTIONS_PREFIX))
        && input_texts
            .next()
            .is_some_and(|text| text.starts_with(ENVIRONMENT_CONTEXT_PREFIX))
}

fn search_text_from_content(
    content: &[CodexMessageContentPart],
    extract: impl Fn(&CodexMessageContentPart) -> Option<&str>,
) -> Option<String> {
    let mut search_text = SearchTextBuilder::default();
    for snippet in content.iter().filter_map(extract) {
        search_text.push(snippet);
    }
    let message = search_text.build("");
    (!message.is_empty()).then_some(message)
}

#[derive(Debug, Deserialize)]
struct CodexTimestampedLine {
    timestamp: Timestamp,
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::tempdir;
    use test_that::prelude::*;

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

        let codex_session_result = parse(
            &content,
            "rollout-2026-03-20T07-30-20-019d09f0-0d96-7e23-94cd-1f6aad7cdc09",
        );
        assert_that!(codex_session_result.as_ref().map(|_| ()), ok(eq(())));
        let codex_session = codex_session_result.expect("Codex session should parse");
        let session = codex_session.into_session(workspace.join("session.jsonl"));
        assert_that!(session.agent, eq(Agent::Codex));
        assert_that!(
            session.name,
            eq("rollout-2026-03-20T07-30-20-019d09f0-0d96-7e23-94cd-1f6aad7cdc09")
        );
        assert_that!(
            session.search_text,
            eq("rollout-2026-03-20T07-30-20-019d09f0-0d96-7e23-94cd-1f6aad7cdc09")
        );
        assert_that!(session.workspace, eq(workspace));
    }

    #[test]
    fn test_parse_codex_session_indexes_user_and_assistant_text_and_updated_at() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:31:20.312Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"why can't I jump with rust-analyzer to these types?\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:32:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Because that symbol is re-exported.\"},{\"type\":\"input_text\",\"text\":\"ignored\"}]}}\n",
            "{\"timestamp\":\"2026-03-20T06:33:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"reasoning\",\"text\":\"hidden\"}}\n"
        );

        let session_result = parse(content, "fallback-name");
        assert_that!(session_result.as_ref().map(|_| ()), ok(eq(())));
        let session = session_result.expect("Codex session should parse");
        assert_that!(session.name, eq("why can't I jump with rust-analyzer to these types?"));
        assert_that!(
            session.search_text,
            eq("why can't I jump with rust-analyzer to these types? Because that symbol is re-exported.")
        );
        assert_that!(
            session.updated_at,
            eq("2026-03-20T06:33:20.312Z".parse::<Timestamp>().unwrap())
        );
    }

    #[test]
    fn test_parse_codex_session_when_user_prompt_is_response_item_uses_it_as_name() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:31:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"developer\",\"content\":[{\"type\":\"input_text\",\"text\":\"developer instructions\"}]}}\n",
            "{\"timestamp\":\"2026-03-20T06:32:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"# AGENTS.md instructions for /tmp/workspace\\n\\n<INSTRUCTIONS>\\nGenerated context\"},{\"type\":\"input_text\",\"text\":\"<environment_context>generated context</environment_context>\"}]}}\n",
            "{\"timestamp\":\"2026-03-20T06:32:21.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"first user prompt\"}]}}\n"
        );

        assert_that!(
            parse(content, "fallback-name"),
            ok(result_of!(
                |session: &CodexSession| session.name.as_str(),
                eq("first user prompt")
            ))
        );
        assert_that!(
            parse(content, "fallback-name"),
            ok(result_of!(
                |session: &CodexSession| session.search_text.as_str(),
                eq("first user prompt")
            ))
        );
    }

    #[test]
    fn test_parse_codex_session_when_prompt_starts_with_agents_prefix_keeps_it() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:31:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"# AGENTS.md instructions need a review\"}]}}\n"
        );

        assert_that!(
            parse(content, "fallback-name"),
            ok(result_of!(
                |session: &CodexSession| session.name.as_str(),
                eq("# AGENTS.md instructions need a review")
            ))
        );
    }

    #[test]
    fn test_parse_codex_session_when_user_input_text_is_not_string_uses_fallback_name() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n",
            "{\"timestamp\":\"2026-03-20T06:31:20.312Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":{\"structured\":\"content\"}}]}}\n"
        );

        assert_that!(
            parse(content, "fallback-name"),
            ok(result_of!(
                |session: &CodexSession| session.name.as_str(),
                eq("fallback-name")
            ))
        );
    }

    #[test]
    fn test_parse_codex_session_when_source_is_subagent_marks_session() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\",\"source\":{\"subagent\":{\"other\":\"guardian\"}}}}\n",
            "{\"timestamp\":\"2026-03-20T06:31:20.312Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"The following is the Codex agent history\"}}\n"
        );

        assert_that!(
            parse(content, "fallback-name"),
            ok(result_of!(
                |session: &CodexSession| session.is_subagent,
                predicate(|is_subagent: &bool| *is_subagent)
                    .with_description("is marked as a subagent", "is not marked as a subagent")
            ))
        );
    }

    #[test]
    fn test_parse_preview_when_source_is_subagent_stops_before_invalid_tail() {
        let content = concat!(
            "{\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\",\"source\":{\"subagent\":{\"other\":\"guardian\"}}}}\n",
            "not json\n"
        );

        assert_that!(
            parse_preview(Cursor::new(content), "fallback-name"),
            ok(result_of!(
                |session: &CodexSession| session.is_subagent,
                predicate(|is_subagent: &bool| *is_subagent)
                    .with_description("is marked as a subagent", "is not marked as a subagent")
            ))
        );
    }

    #[test]
    fn test_parse_codex_session_with_invalid_scanned_line_returns_error() {
        let content = "{\"timestamp\":\"not-a-date\",\"type\":\"session_meta\",\"payload\":{\"id\":\"019d09f0-0d96-7e23-94cd-1f6aad7cdc09\",\"timestamp\":\"2026-03-20T06:30:20.312Z\",\"cwd\":\"/tmp/workspace\"}}\n";

        assert_that!(
            parse(content, "fallback-name").map(|_| ()),
            err(displays_as(contains_substring(
                "failed to parse Codex session json line"
            )))
        );
    }
}
