use std::path::PathBuf;

use jiff::Timestamp;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;

use crate::agent::Agent;
use crate::agent::session::SearchTextBuilder;
use crate::agent::session::Session;

/// Parse Cursor chat `meta.json` into a session.
///
/// # Errors
/// Returns an error when the JSON is invalid, `cwd` is empty, or a timestamp is out of range.
pub fn parse_chat_meta(json: &str, session_id: String) -> rootcause::Result<CursorSession> {
    let doc = serde_json::from_str::<CursorChatMeta>(json)
        .context("failed to parse Cursor chat metadata".to_owned())
        .attach(format!("meta_json={json}"))?;

    let cwd = doc.cwd.trim();
    if cwd.is_empty() {
        return Err(report!("Cursor chat meta cwd is empty").attach(format!("session_id={session_id}")));
    }
    let workspace_dir = PathBuf::from(cwd);

    let created_at = Timestamp::from_millisecond(doc.created_at_ms)
        .context("Cursor createdAtMs is out of range".to_owned())
        .attach(format!("session_id={session_id}"))
        .attach(format!("created_at_ms={}", doc.created_at_ms))?;
    let updated_at = match doc.updated_at_ms {
        Some(updated_at_ms) => Timestamp::from_millisecond(updated_at_ms)
            .context("Cursor updatedAtMs is out of range".to_owned())
            .attach(format!("session_id={session_id}"))
            .attach(format!("updated_at_ms={updated_at_ms}"))?,
        None => created_at,
    };

    let name = doc.title.filter(|title| !title.trim().is_empty()).unwrap_or_else(|| {
        workspace_dir
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map_or_else(|| session_id.clone(), str::to_owned)
    });

    Ok(CursorSession {
        id: session_id,
        name: name.clone(),
        search_text: name,
        workspace: workspace_dir,
        created_at,
        updated_at,
        has_conversation: doc.has_conversation.unwrap_or(true),
    })
}

pub fn build_search_text_from_prompts(session_name: &str, prompts: &[String]) -> String {
    let mut search_text = SearchTextBuilder::default();
    for prompt in prompts {
        search_text.push(prompt);
    }
    search_text.build(session_name)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorSession {
    pub id: String,
    pub name: String,
    pub search_text: String,
    pub workspace: PathBuf,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub has_conversation: bool,
}

impl CursorSession {
    pub fn into_session(self, path: PathBuf) -> Session {
        let mut session = Session::new(Agent::Cursor, self.id, self.workspace, path, None, self.created_at);
        session.name = self.name;
        session.search_text = self.search_text;
        session.updated_at = self.updated_at;
        session
    }
}

#[derive(Debug, Deserialize)]
struct CursorChatMeta {
    #[serde(rename = "createdAtMs")]
    created_at_ms: i64,
    #[serde(rename = "updatedAtMs")]
    updated_at_ms: Option<i64>,
    title: Option<String>,
    cwd: String,
    #[serde(rename = "hasConversation")]
    has_conversation: Option<bool>,
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_parse_chat_meta_when_json_has_cwd_returns_workspace_and_title() {
        let json = r#"{"schemaVersion":1,"createdAtMs":1774877738013,"hasConversation":true,"title":"Status Line","updatedAtMs":1774877739013,"cwd":"/Users/gianlu/data/dev/work/pws-api/pws-api"}"#;

        let cursor_session_result = parse_chat_meta(json, "session-id".to_owned());
        assert_that!(cursor_session_result.as_ref().map(|_| ()), ok(eq(())));
        let cursor_session = cursor_session_result.expect("Cursor chat metadata should parse");
        let session = cursor_session.into_session(PathBuf::from("session-id"));

        assert_that!(session.agent, eq(Agent::Cursor));
        assert_that!(session.id, eq("session-id"));
        assert_that!(session.name, eq("Status Line"));
        assert_that!(
            session.workspace,
            eq(PathBuf::from("/Users/gianlu/data/dev/work/pws-api/pws-api"))
        );
    }

    #[test]
    fn test_parse_chat_meta_when_cwd_is_empty_returns_error() {
        let json = r#"{"schemaVersion":1,"createdAtMs":1774877738013,"hasConversation":true,"title":"Status Line","cwd":"  "}"#;

        assert_that!(
            (parse_chat_meta(json, "session-id".to_owned())).map(|_| ()),
            err(displays_as(contains_substring("Cursor chat meta cwd is empty")))
        );
    }

    #[test]
    fn test_build_search_text_from_prompts_when_prompts_repeat_keeps_unique_snippets() {
        let search_text = build_search_text_from_prompts(
            "Status Line",
            &[
                "first prompt".to_owned(),
                "first prompt".to_owned(),
                "second prompt".to_owned(),
            ],
        );

        assert_that!(search_text, eq("Status Line first prompt second prompt"));
    }
}
