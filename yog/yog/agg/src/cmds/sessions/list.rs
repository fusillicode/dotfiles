use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use jiff::Timestamp;
use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Serialize;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_agents::agent::Agent;
use ytil_agents::agent::session::Session;
use ytil_agents::agent::session::SessionKey;

pub fn run() -> rootcause::Result<()> {
    let sessions = load_sorted_sessions()?;

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    let renderable_sessions = RenderableSession::from_sessions(sessions);
    let Some(selected) = ytil_tui::minimal_multi_select(renderable_sessions, ToString::to_string, |session| {
        session.session.search_text.clone()
    })?
    else {
        println!("No sessions selected");
        return Ok(());
    };

    let Some(op) = ytil_tui::minimal_select::<Op>(Op::iter().collect())? else {
        println!("No action selected");
        return Ok(());
    };

    match op {
        Op::Resume => ytil_tui::require_single(&selected, "sessions").and_then(launch_session),
        Op::Delete => {
            for session in &selected {
                delete_session(session)?;
            }
            Ok(())
        }
    }
}

pub fn run_json(args: &[String]) -> rootcause::Result<()> {
    let session_keys = parse_json_session_keys(args)?;
    let sessions = load_sorted_sessions_by_key(&session_keys)?;
    let home_dir = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("/"), std::path::PathBuf::from);
    let rows = RenderableSession::from_sessions(sessions)
        .into_iter()
        .map(|session| JsonSession::new(&session, &home_dir))
        .collect::<rootcause::Result<Vec<_>>>()?;

    println!(
        "{}",
        serde_json::to_string(&rows).context("failed to serialize sessions")?
    );
    Ok(())
}

fn parse_json_session_keys(args: &[String]) -> rootcause::Result<Vec<SessionKey>> {
    let mut session_keys = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--session" => {
                let Some(key) = args.next() else {
                    return Err(report!("missing --session value"));
                };
                session_keys.push(key.parse()?);
            }
            unexpected => {
                return Err(report!("unknown agg sessions list --json arg").attach(format!("arg={unexpected}")));
            }
        }
    }
    if session_keys.is_empty() {
        return Err(report!("agg sessions list --json requires at least one --session"));
    }
    session_keys.sort();
    session_keys.dedup();
    Ok(session_keys)
}

fn load_sorted_sessions() -> rootcause::Result<Vec<Session>> {
    let mut sessions = Vec::new();
    sessions.extend(ytil_agents::agent::session_loader::load_sessions()?);
    sort_sessions(&mut sessions);
    Ok(sessions)
}

fn load_sorted_sessions_by_key(keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let mut sessions = ytil_agents::agent::session_loader::load_sessions_by_key(keys)?;
    sort_sessions(&mut sessions);
    Ok(sessions)
}

fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.created_at.cmp(&a.created_at))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
}

struct RenderableSession {
    session: Session,
    branch: Option<String>,
}

impl RenderableSession {
    fn from_sessions(sessions: Vec<Session>) -> Vec<Self> {
        let mut timestamps_by_workspace = HashMap::<std::path::PathBuf, Vec<(usize, Timestamp)>>::new();
        for (index, session) in sessions.iter().enumerate() {
            timestamps_by_workspace
                .entry(session.workspace.clone())
                .or_default()
                .push((index, session.created_at));
        }

        let mut branches = vec![None; sessions.len()];
        for (workspace, timestamp_entries) in timestamps_by_workspace {
            let timestamps: Vec<Timestamp> = timestamp_entries.iter().map(|(_, timestamp)| *timestamp).collect();
            for ((index, _), branch) in timestamp_entries
                .into_iter()
                .zip(ytil_git::branch::get_at_many(&workspace, &timestamps))
            {
                if let Some(branch_slot) = branches.get_mut(index) {
                    *branch_slot = branch;
                }
            }
        }

        sessions
            .into_iter()
            .zip(branches)
            .map(|(session, branch)| Self { session, branch })
            .collect()
    }

    fn can_resume(&self) -> bool {
        self.session.workspace.is_dir()
    }

    fn workspace_status(&self) -> &'static str {
        if self.can_resume() { "" } else { " [missing workspace]" }
    }

    fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    fn plain_summary(&self, home_dir: &Path) -> String {
        let path_label = ytil_tui::short_path(&self.session.workspace, home_dir);
        let session_name = ytil_tui::display_fixed_width(&self.session.name, 42);
        let updated_label = self.session.updated_at.strftime("%d/%m/%Y-%H:%M").to_string();
        let created_label = self.session.created_at.strftime("%d/%m/%Y-%H:%M").to_string();
        let agent = self.session.agent.short_name();

        self.branch().map_or_else(
            || {
                format!(
                    "{agent} {path_label}{} {session_name} {updated_label} {created_label}",
                    self.workspace_status()
                )
            },
            |branch| {
                format!(
                    "{agent} {path_label} {branch}{} {session_name} {updated_label} {created_label}",
                    self.workspace_status()
                )
            },
        )
    }
}

impl Display for RenderableSession {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let agent_name = match self.session.agent {
            Agent::Claude => self.session.agent.short_name().red().bold().to_string(),
            Agent::Codex => self.session.agent.short_name().green().bold().to_string(),
            Agent::Cursor => self.session.agent.short_name().bright_black().bold().to_string(),
            Agent::Gemini | Agent::Opencode => self.session.agent.short_name().bold().to_string(),
        };

        let path_label = ytil_tui::short_path(
            &self.session.workspace,
            std::env::var_os("HOME")
                .as_deref()
                .map_or_else(|| std::path::Path::new("/"), std::path::Path::new),
        );
        let session_name = ytil_tui::display_fixed_width(&self.session.name, 42);
        let updated_label = self.session.updated_at.strftime("%d/%m/%Y-%H:%M").to_string();
        let created_label = self.session.created_at.strftime("%d/%m/%Y-%H:%M").to_string();

        if let Some(branch) = self.branch() {
            write!(
                f,
                "{agent_name} {} {}{} {} {} {}",
                path_label.cyan().bold(),
                branch.white(),
                self.workspace_status().yellow(),
                session_name.dimmed().bold(),
                updated_label.blue(),
                created_label.blue(),
            )
        } else {
            write!(
                f,
                "{agent_name} {}{} {} {} {}",
                path_label.cyan().bold(),
                self.workspace_status().yellow(),
                session_name.dimmed().bold(),
                updated_label.blue(),
                created_label.blue(),
            )
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonSession {
    agent: &'static str,
    workspace: std::path::PathBuf,
    session_id: String,
    summary: String,
    display: String,
    search: String,
    updated_at: Timestamp,
    resume_program: String,
    resume_args: Vec<String>,
}

impl JsonSession {
    fn new(session: &RenderableSession, home_dir: &Path) -> rootcause::Result<Self> {
        let display = session.plain_summary(home_dir);
        let search = search_corpus(&display, &session.session.search_text);
        let (resume_program, resume_args) = session.session.build_resume_command()?;
        Ok(Self {
            agent: session.session.agent.name(),
            workspace: session.session.workspace.clone(),
            session_id: session.session.id.clone(),
            summary: session.session.name.clone(),
            display,
            search,
            updated_at: session.session.updated_at,
            resume_program: resume_program.to_string(),
            resume_args,
        })
    }
}

fn search_corpus(display_text: &str, hidden_search: &str) -> String {
    let visible_match_text = normalize_search(display_text);
    let hidden_search = normalize_search(hidden_search);
    if hidden_search.is_empty() || hidden_search == visible_match_text {
        visible_match_text
    } else {
        format!("{visible_match_text} {hidden_search}")
    }
}

fn normalize_search(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, EnumIter)]
enum Op {
    Resume,
    Delete,
}

impl Display for Op {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resume => write!(f, "{}", "Resume".green().bold()),
            Self::Delete => write!(f, "{}", "Delete".red().bold()),
        }
    }
}

fn launch_session(session: &RenderableSession) -> rootcause::Result<()> {
    if !session.can_resume() {
        return Err(report!("cannot resume session because its workspace is missing")
            .attach(format!("workspace={}", session.session.workspace.display()))
            .attach(format!("session_id={}", session.session.id)));
    }

    let session = &session.session;
    let (program, args) = session.build_resume_command()?;

    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(&session.workspace)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    #[cfg(unix)]
    {
        let error = cmd.exec();
        Err(report!("failed to exec agent CLI")
            .attach(format!("error={error}"))
            .attach(format!("agent={}", session.agent.name()))
            .attach(format!("workspace={}", session.workspace.display()))
            .attach(format!("session_id={}", session.id)))
    }

    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .context("failed to launch agent CLI")
            .attach_with(|| format!("agent={}", session.agent.name()))
            .attach_with(|| format!("workspace={}", session.workspace.display()))
            .attach_with(|| format!("session_id={}", session.id))?;

        if !status.success() {
            return Err(report!("agent CLI exited with non-zero status")
                .attach_with(|| format!("agent={}", session.agent.name()))
                .attach_with(|| format!("workspace={}", session.workspace.display()))
                .attach_with(|| format!("session_id={}", session.id))
                .attach_with(|| format!("status={status}")));
        }

        Ok(())
    }
}

fn delete_session(session: &RenderableSession) -> rootcause::Result<()> {
    let delete_path = &session.session.path;
    if delete_path.is_dir() {
        std::fs::remove_dir_all(delete_path)
            .context("failed to delete session directory")
            .attach_with(|| format!("path={}", delete_path.display()))
            .attach_with(|| format!("session_id={}", session.session.id))?;
    } else {
        std::fs::remove_file(delete_path)
            .context("failed to delete session file")
            .attach_with(|| format!("path={}", delete_path.display()))
            .attach_with(|| format!("session_id={}", session.session.id))?;
    }
    println!("{} {session}", "Deleted".red().bold());
    Ok(())
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_search_corpus_matches_agg_visible_plus_hidden_filtering() {
        let display = "cx  ~/repo   branch   session name  09/05/2026-10:00";
        let hidden = "first user prompt\nassistant reply";

        let search = search_corpus(display, hidden);

        assert_that!(
            search,
            eq("cx ~/repo branch session name 09/05/2026-10:00 first user prompt assistant reply")
        );
    }

    #[test]
    fn test_json_session_renders_plain_agg_summary_and_resume_command() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("repo");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let created_at = Timestamp::from_second(1_700_000_000).expect("test timestamp should be valid");
        let updated_at = Timestamp::from_second(1_700_000_100).expect("test timestamp should be valid");
        let session = Session {
            id: "session-id".to_string(),
            agent: Agent::Codex,
            name: "fix issue".to_string(),
            search_text: "hidden prompt".to_string(),
            workspace: workspace.clone(),
            path: dir.path().join("session.jsonl"),
            created_at,
            updated_at,
        };
        let renderable_sessions = RenderableSession::from_sessions(vec![session]);
        assert_that!(renderable_sessions.len(), eq(1));
        let renderable = &renderable_sessions[0];

        assert_that!(
            JsonSession::new(renderable, dir.path()),
            ok(all!(
                result_of!(
                    |row: &JsonSession| row.display.as_str(),
                    starts_with("cx ~/repo fix issue")
                ),
                result_of!(
                    |row: &JsonSession| row.search.as_str(),
                    contains_substring("hidden prompt")
                ),
                result_of!(|row: &JsonSession| row.agent, eq("codex")),
                result_of!(|row: &JsonSession| &row.workspace, points_to(eq(workspace))),
                result_of!(|row: &JsonSession| row.session_id.as_str(), eq("session-id")),
                result_of!(|row: &JsonSession| row.summary.as_str(), eq("fix issue")),
                result_of!(|row: &JsonSession| row.updated_at, eq(updated_at)),
                result_of!(|row: &JsonSession| row.resume_program.as_str(), eq("codex")),
                result_of!(
                    |row: &JsonSession| row.resume_args.first().map(String::as_str),
                    eq(Some("resume"))
                ),
            ))
        );
    }

    #[test]
    fn test_parse_json_session_keys_requires_at_least_one_session_key() {
        assert_that!(
            (parse_json_session_keys(&[])).map(|_| ()),
            err(displays_as(contains_substring("requires at least one --session")))
        );
    }

    #[test]
    fn test_parse_json_session_keys_parses_and_dedupes_requested_session_keys() {
        assert_that!(
            parse_json_session_keys(&[
                String::from("--session"),
                String::from("codex:target"),
                String::from("--session"),
                String::from("codex:target"),
            ]),
            ok(eq([SessionKey::new(Agent::Codex, "target")]))
        );
    }
}
