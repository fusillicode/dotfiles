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
                vec!["resume".into(), self.id.clone(), "--cd".into(), workspace.into()],
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

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn build_resume_command_matches_agent() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let claude = Session {
            agent: Agent::Claude,
            id: "session-id".into(),
            workspace: workspace.clone(),
            name: "session-name".into(),
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
}
