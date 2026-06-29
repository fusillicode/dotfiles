use std::fmt::Display;
use std::fmt::Formatter;
use std::path::PathBuf;
use std::str::FromStr;

use chrono::DateTime;
use chrono::Utc;
use rootcause::option_ext::OptionExt;
use rootcause::report;

use crate::agent::Agent;

const SEARCH_TEXT_MAX_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionKey {
    agent: Agent,
    id: String,
}

impl SessionKey {
    pub fn new(agent: Agent, id: impl Into<String>) -> Self {
        Self { agent, id: id.into() }
    }

    pub const fn agent(&self) -> Agent {
        self.agent
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

impl Display for SessionKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.agent.name(), self.id)
    }
}

impl FromStr for SessionKey {
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((agent, id)) = value.split_once(':') else {
            return Err(report!("invalid session key").attach(format!("value={value}")));
        };
        let agent =
            Agent::from_name(agent).map_err(|err| report!("invalid session key agent").attach(err.to_string()))?;
        if id.is_empty() {
            return Err(report!("invalid session key").attach(format!("value={value}")));
        }
        Ok(Self::new(agent, id))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub name: String,
    pub search_text: String,
    pub workspace: PathBuf,
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Session {
    pub fn new(
        agent: Agent,
        session_id: String,
        workspace_dir: PathBuf,
        path: PathBuf,
        name: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        let name = name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| {
            workspace_dir
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map_or_else(|| session_id.clone(), str::to_owned)
        });

        Self {
            id: session_id,
            agent,
            search_text: name.clone(),
            name,
            workspace: workspace_dir,
            path,
            created_at,
            updated_at: created_at,
        }
    }

    /// Build the argv required to resume this session with its owning agent CLI.
    ///
    /// # Errors
    /// Returns an error when the workspace path is not UTF-8 or the agent has no
    /// supported resume command.
    pub fn build_resume_command(&self) -> rootcause::Result<(&'static str, Vec<String>)> {
        let workspace = self.workspace.to_str().context("non-utf8 workspace dir".to_owned())?;
        match self.agent {
            Agent::Claude => Ok(("claude", vec!["--resume".into(), self.id.clone()])),
            Agent::Codex => Ok((
                "codex",
                self.build_codex_resume_args(workspace, std::env::var_os("ZELLIJ").is_some()),
            )),
            Agent::Cursor => Ok((
                "cursor-agent",
                vec![
                    "--resume".into(),
                    self.id.clone(),
                    "--workspace".into(),
                    workspace.into(),
                ],
            )),
            Agent::Gemini | Agent::Opencode => {
                Err(report!("resume is not supported for this agent").attach(format!("agent={}", self.agent)))
            }
        }
    }

    fn build_codex_resume_args(&self, workspace: &str, is_zellij: bool) -> Vec<String> {
        let mut args = vec!["resume".into(), self.id.clone()];
        // In Zellij, Codex's mouse-aware TUI captures wheel events before
        // Zellij can use them for inline scrollback. Keep inline mode only
        // outside Zellij, where terminal scrollback works as intended.
        if !is_zellij {
            args.push("--no-alt-screen".into());
        }
        args.extend(["--cd".into(), workspace.into()]);
        args
    }
}

#[derive(Debug, Default)]
pub struct SearchTextBuilder {
    snippets_text: String,
    first_snippet: Option<String>,
    last_snippet: Option<String>,
    reached_limit: bool,
}

impl SearchTextBuilder {
    pub fn push(&mut self, raw: &str) {
        if self.reached_limit {
            return;
        }

        let snippet = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let Some(snippet) = (!snippet.is_empty()).then_some(snippet) else {
            return;
        };
        if self.last_snippet.as_ref().is_some_and(|last| last == &snippet) {
            return;
        }
        if self.first_snippet.is_none() {
            self.first_snippet = Some(snippet.clone());
        }

        self.reached_limit = !push_normalized_snippet(&mut self.snippets_text, &mut self.last_snippet, &snippet);
    }

    pub fn build(self, fallback: &str) -> String {
        let fallback = fallback.split_whitespace().collect::<Vec<_>>().join(" ");
        let Some(fallback) = (!fallback.is_empty()).then_some(fallback) else {
            return self.snippets_text;
        };

        if self.first_snippet.as_ref().is_some_and(|first| first == &fallback) {
            return self.snippets_text;
        }

        let mut search_text = String::new();
        let mut last_snippet = None::<String>;
        if !push_normalized_snippet(&mut search_text, &mut last_snippet, &fallback) {
            return search_text;
        }
        if self.snippets_text.is_empty() {
            return search_text;
        }

        let separator_len = usize::from(!search_text.is_empty());
        if search_text.len().saturating_add(separator_len) >= SEARCH_TEXT_MAX_BYTES {
            return search_text;
        }
        if !search_text.is_empty() {
            search_text.push(' ');
        }

        let remaining = SEARCH_TEXT_MAX_BYTES.saturating_sub(search_text.len());
        if let Some(truncated) = truncate_to_boundary(&self.snippets_text, remaining) {
            search_text.push_str(truncated);
        }

        search_text
    }
}

fn push_normalized_snippet(search_text: &mut String, last_snippet: &mut Option<String>, snippet: &str) -> bool {
    let separator_len = usize::from(!search_text.is_empty());
    if search_text.len().saturating_add(separator_len) >= SEARCH_TEXT_MAX_BYTES {
        return false;
    }
    if !search_text.is_empty() {
        search_text.push(' ');
    }

    let remaining = SEARCH_TEXT_MAX_BYTES.saturating_sub(search_text.len());
    if remaining == 0 {
        return false;
    }

    let snippet_len = snippet.len();
    truncate_to_boundary(snippet, remaining).is_some_and(|truncated| {
        let is_full_snippet = truncated.len() == snippet_len;
        search_text.push_str(truncated);
        *last_snippet = Some(snippet.to_owned());
        is_full_snippet
    })
}

fn truncate_to_boundary(text: &str, max_bytes: usize) -> Option<&str> {
    if max_bytes == 0 {
        return None;
    }
    if text.len() <= max_bytes {
        return Some(text);
    }

    let mut end = 0;
    for (idx, ch) in text.char_indices() {
        let next = idx.saturating_add(ch.len_utf8());
        if next > max_bytes {
            break;
        }
        end = next;
    }

    (end > 0).then(|| text.get(..end)).flatten()
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_session_key_string_round_trip_uses_agent_session_format() {
        let key_result = "codex:session-id".parse::<SessionKey>();
        assert_that!(key_result, ok(anything()));
        let key = key_result.expect("session key should parse");

        assert_that!(key, eq(SessionKey::new(Agent::Codex, "session-id")));
        assert_that!(key.to_string(), eq("codex:session-id"));
    }

    #[test]
    fn test_build_resume_command_matches_agent() {
        let tempdir = tempdir().expect("tempdir should be created");
        let workspace = tempdir.path().join("workspace");
        let path = tempdir.path().join("session.jsonl");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let created_at = DateTime::from_timestamp_millis(1).expect("test timestamp should be valid");

        let claude = Session {
            agent: Agent::Claude,
            id: "session-id".into(),
            workspace: workspace.clone(),
            name: "session-name".into(),
            search_text: "session-name".into(),
            path,
            created_at: created_at.to_utc(),
            updated_at: created_at.to_utc(),
        };
        let codex = Session {
            agent: Agent::Codex,
            ..claude.clone()
        };
        let cursor = Session {
            agent: Agent::Cursor,
            ..claude.clone()
        };

        let claude_command_result = claude.build_resume_command();
        assert_that!(claude_command_result, ok(anything()));
        let (_, claude_args) = claude_command_result.expect("Claude session should build resume command");
        assert_that!(claude_args, eq(vec!["--resume".to_owned(), "session-id".to_owned()]));
        let workspace_str = workspace.to_str().expect("workspace test path should be utf8");
        assert_that!(
            codex.build_codex_resume_args(workspace_str, false),
            eq(vec![
                "resume".to_owned(),
                "session-id".to_owned(),
                "--no-alt-screen".to_owned(),
                "--cd".to_owned(),
                workspace_str.to_owned(),
            ])
        );
        assert_that!(
            codex.build_codex_resume_args(workspace_str, true),
            eq(vec![
                "resume".to_owned(),
                "session-id".to_owned(),
                "--cd".to_owned(),
                workspace_str.to_owned(),
            ])
        );
        let cursor_command_result = cursor.build_resume_command();
        assert_that!(cursor_command_result, ok(anything()));
        let (_, cursor_args) = cursor_command_result.expect("Cursor session should build resume command");
        assert_that!(
            cursor_args,
            eq(vec![
                "--resume".to_owned(),
                "session-id".to_owned(),
                "--workspace".to_owned(),
                workspace_str.to_owned(),
            ])
        );
    }

    #[test]
    fn test_session_new_sets_search_text_from_resolved_name() {
        let tempdir = tempdir().expect("tempdir should be created");
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let created_at = DateTime::from_timestamp_millis(1).expect("test timestamp should be valid");

        let session = Session::new(
            Agent::Codex,
            "session-id".into(),
            workspace,
            PathBuf::from("session.jsonl"),
            Some("hello world".into()),
            created_at.to_utc(),
        );

        assert_that!(session.name, eq("hello world"));
        assert_that!(session.search_text, eq("hello world"));
    }

    #[test]
    fn test_search_text_builder_normalizes_dedupes_and_falls_back() {
        let mut builder = SearchTextBuilder::default();
        for snippet in ["  fallback  ", "first\nline", "", "first line", "second\tline"] {
            builder.push(snippet);
        }
        let search_text = builder.build("fallback");

        assert_that!(search_text, eq("fallback first line second line"));
    }
}
