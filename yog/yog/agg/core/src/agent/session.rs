use std::path::PathBuf;

use chrono::DateTime;
use chrono::Utc;
use rootcause::option_ext::OptionExt as _;
use rootcause::report;

use crate::agent::Agent;

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

const SEARCH_TEXT_MAX_BYTES: usize = 32 * 1024;

impl Session {
    pub fn new(
        agent: Agent,
        session_id: String,
        workspace_dir: PathBuf,
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
            path: PathBuf::new(),
            created_at,
            updated_at: created_at,
        }
    }

    pub fn build_resume_command(&self) -> rootcause::Result<(&'static str, Vec<String>)> {
        let workspace = self.workspace.to_str().context("non-utf8 workspace dir".to_owned())?;
        match self.agent {
            Agent::Claude => Ok(("claude", vec!["--resume".into(), self.id.clone()])),
            Agent::Codex => Ok((
                "codex",
                vec![
                    "resume".into(),
                    self.id.clone(),
                    "--no-alt-screen".into(),
                    "--cd".into(),
                    workspace.into(),
                ],
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
}

#[derive(Debug, Default)]
pub(crate) struct SearchTextBuilder {
    snippets_text: String,
    first_snippet: Option<String>,
    last_snippet: Option<String>,
    reached_limit: bool,
}

impl SearchTextBuilder {
    pub(crate) fn push(&mut self, raw: &str) {
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

        self.reached_limit = !push_normalized_snippet(&mut self.snippets_text, &mut self.last_snippet, snippet);
    }

    pub(crate) fn build(self, fallback: &str) -> String {
        let fallback = fallback.split_whitespace().collect::<Vec<_>>().join(" ");
        let Some(fallback) = (!fallback.is_empty()).then_some(fallback) else {
            return self.snippets_text;
        };

        if self.first_snippet.as_ref().is_some_and(|first| first == &fallback) {
            return self.snippets_text;
        }

        let mut search_text = String::new();
        let mut last_snippet = None::<String>;
        if !push_normalized_snippet(&mut search_text, &mut last_snippet, fallback) {
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

fn push_normalized_snippet(search_text: &mut String, last_snippet: &mut Option<String>, snippet: String) -> bool {
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
    if let Some(truncated) = truncate_to_boundary(&snippet, remaining) {
        let is_full_snippet = truncated.len() == snippet_len;
        search_text.push_str(truncated);
        *last_snippet = Some(snippet);
        is_full_snippet
    } else {
        false
    }
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

    (end > 0).then_some(&text[..end])
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_build_resume_command_matches_agent() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let claude = Session {
            agent: Agent::Claude,
            id: "session-id".into(),
            workspace: workspace.clone(),
            name: "session-name".into(),
            search_text: "session-name".into(),
            path: PathBuf::new(),
            created_at: DateTime::from_timestamp_millis(1).unwrap().to_utc(),
            updated_at: DateTime::from_timestamp_millis(1).unwrap().to_utc(),
        };
        let codex = Session {
            agent: Agent::Codex,
            ..claude.clone()
        };
        let cursor = Session {
            agent: Agent::Cursor,
            ..claude.clone()
        };

        assert2::assert!(let Ok((_, claude_args)) = claude.build_resume_command());
        pretty_assertions::assert_eq!(claude_args, vec!["--resume".to_owned(), "session-id".to_owned()]);
        assert2::assert!(let Some(workspace_str) = workspace.to_str());
        assert2::assert!(let Ok((_, codex_args)) = codex.build_resume_command());
        pretty_assertions::assert_eq!(
            codex_args,
            vec![
                "resume".to_owned(),
                "session-id".to_owned(),
                "--no-alt-screen".to_owned(),
                "--cd".to_owned(),
                workspace_str.to_owned(),
            ]
        );
        assert2::assert!(let Ok((_, cursor_args)) = cursor.build_resume_command());
        pretty_assertions::assert_eq!(
            cursor_args,
            vec![
                "--resume".to_owned(),
                "session-id".to_owned(),
                "--workspace".to_owned(),
                workspace_str.to_owned(),
            ]
        );
    }

    #[test]
    fn test_session_new_sets_search_text_from_resolved_name() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let session = Session::new(
            Agent::Codex,
            "session-id".into(),
            workspace,
            Some("hello world".into()),
            DateTime::from_timestamp_millis(1).unwrap().to_utc(),
        );

        pretty_assertions::assert_eq!(session.name, "hello world");
        pretty_assertions::assert_eq!(session.search_text, "hello world");
    }

    #[test]
    fn test_search_text_builder_normalizes_dedupes_and_falls_back() {
        let mut builder = SearchTextBuilder::default();
        for snippet in ["  fallback  ", "first\nline", "", "first line", "second\tline"] {
            builder.push(snippet);
        }
        let search_text = builder.build("fallback");

        pretty_assertions::assert_eq!(search_text, "fallback first line second line");
    }
}
