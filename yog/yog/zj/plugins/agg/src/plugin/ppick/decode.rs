use std::path::Path;

use agg::AGENTS_PIPE;
use agg::GitStat;
use ytil_agents::agent::AgentEventPayload;
use zellij_tile::prelude::PipeMessage;

use crate::plugin::ppick::state::SessionEntry;

pub fn agent_event_from_pipe(msg: &PipeMessage) -> Option<AgentEventPayload> {
    if msg.name != AGENTS_PIPE {
        return None;
    }

    let pane_id = msg.args.get("pane_id")?;
    let agent = msg.args.get("agent")?;
    let payload = msg.payload.as_deref().unwrap_or("");
    AgentEventPayload::parse(pane_id, agent, payload)
        .inspect_err(|error| eprintln!("agg ppick: {error}"))
        .ok()
}

pub fn git_stat_from_run_command(requested_cwd: &Path, exit_code: Option<i32>, stdout: &[u8]) -> Option<GitStat> {
    if exit_code != Some(0) {
        return None;
    }

    let output = String::from_utf8_lossy(stdout);
    let stats = agg::parse_git_stat_records(&output)
        .inspect_err(|error| eprintln!("agg ppick: {error}"))
        .ok()?;
    stats.into_iter().find(|stat| stat.path.as_path() == requested_cwd)
}

pub fn sessions_from_stdout(stdout: &[u8]) -> serde_json::Result<Vec<SessionEntry>> {
    serde_json::from_slice(stdout)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::AgentEventKind;
    use ytil_agents::agent::AgentEventPayload;
    use zellij_tile::prelude::PipeMessage;
    use zellij_tile::prelude::PipeSource;

    use super::*;

    #[test]
    fn test_agent_event_from_pipe_returns_update_for_agg_agent_pipe() {
        let msg = PipeMessage {
            source: PipeSource::Keybind,
            name: AGENTS_PIPE.to_string(),
            payload: Some(AgentEventKind::Busy.as_str().to_string()),
            args: BTreeMap::from([
                (String::from("pane_id"), String::from("42")),
                (String::from("agent"), String::from("codex")),
            ]),
            is_private: false,
        };

        let event = agent_event_from_pipe(&msg);

        assert_eq!(
            event,
            Some(AgentEventPayload {
                pane_id: 42,
                agent: Agent::Codex,
                kind: AgentEventKind::Busy,
            })
        );
    }

    #[test]
    fn test_git_stat_from_run_command_returns_branch_and_stat_for_matching_requested_cwd() {
        let cwd = PathBuf::from("/tmp/repo");
        let stat = GitStat {
            path: cwd.clone(),
            branch: Some("main".to_string()),
            insertions: 2,
            deletions: 1,
            new_files: 3,
            is_worktree: false,
            ..Default::default()
        };
        let stdout = stat.to_string();

        let parsed = git_stat_from_run_command(&cwd, Some(0), stdout.as_bytes());

        assert_eq!(parsed, Some(stat));
    }

    #[test]
    fn test_git_stat_from_run_command_ignores_other_cwds() {
        let cwd = PathBuf::from("/tmp/repo");
        let stdout = GitStat {
            path: PathBuf::from("/tmp/other"),
            branch: Some("main".to_string()),
            insertions: 2,
            deletions: 1,
            new_files: 3,
            is_worktree: false,
            ..Default::default()
        }
        .to_string();

        let parsed = git_stat_from_run_command(&cwd, Some(0), stdout.as_bytes());

        assert_eq!(parsed, None);
    }

    #[test]
    fn test_sessions_from_stdout_parses_current_ags_json_contract() {
        let stdout = br#"[{"agent":"codex","workspace":"/tmp/repo","session_id":"abc","summary":"how to solve","display":"cx ~/repo fix","search":"hidden prompt","updated_at":"2026-05-09T10:00:00Z","resume_program":"codex","resume_args":["resume","abc"]}]"#;

        let entries = sessions_from_stdout(stdout).unwrap();

        assert_eq!(
            entries,
            vec![SessionEntry {
                agent: "codex".to_string(),
                workspace: PathBuf::from("/tmp/repo"),
                session_id: "abc".to_string(),
                summary: Some("how to solve".to_string()),
                display: "cx ~/repo fix".to_string(),
                search: "hidden prompt".to_string(),
                updated_at: "2026-05-09T10:00:00Z".to_string(),
            }]
        );
    }
}
