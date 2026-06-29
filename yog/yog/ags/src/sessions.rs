use std::cell::RefCell;
use std::fmt::Display;
use std::fmt::Formatter;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Serialize;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_agents::agent::Agent;
use ytil_agents::agent::session::Session;
use ytil_agents::agent::session::SessionKey;

pub fn list_json(args: &[String]) -> rootcause::Result<()> {
    let session_keys = parse_json_session_keys(args)?;
    let sessions = load_sorted_sessions_by_key(&session_keys)?;
    let home_dir = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("/"), std::path::PathBuf::from);
    let rows = sessions
        .into_iter()
        .map(RenderableSession::from)
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
                return Err(report!("unknown ags list --json arg").attach(format!("arg={unexpected}")));
            }
        }
    }
    if session_keys.is_empty() {
        return Err(report!("ags list --json requires at least one --session"));
    }
    session_keys.sort();
    session_keys.dedup();
    Ok(session_keys)
}

pub fn run() -> rootcause::Result<()> {
    let sessions = load_sorted_sessions()?;

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    let renderable_sessions: Vec<RenderableSession> = sessions.into_iter().map(RenderableSession::from).collect();
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
    branch: RefCell<Option<String>>,
}

impl From<Session> for RenderableSession {
    fn from(session: Session) -> Self {
        Self {
            session,
            branch: RefCell::default(),
        }
    }
}

impl RenderableSession {
    fn branch(&self) -> Option<String> {
        if let Some(branch) = self.branch.borrow().as_ref() {
            return Some(branch.to_owned());
        }

        let branch = ytil_git::branch::get_at(&self.session.workspace, self.session.created_at)?;
        *self.branch.borrow_mut() = Some(branch.clone());
        Some(branch)
    }

    fn plain_summary(&self, home_dir: &Path) -> String {
        let path_label = ytil_tui::short_path(&self.session.workspace, home_dir);
        let session_name = ytil_tui::display_fixed_width(&self.session.name, 42);
        let updated_label = self.session.updated_at.format("%d/%m/%Y-%H:%M").to_string();
        let created_label = self.session.created_at.format("%d/%m/%Y-%H:%M").to_string();
        let agent = self.session.agent.short_name();

        self.branch().map_or_else(
            || format!("{agent} {path_label} {session_name} {updated_label} {created_label}"),
            |branch| format!("{agent} {path_label} {branch} {session_name} {updated_label} {created_label}"),
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
        let updated_label = self.session.updated_at.format("%d/%m/%Y-%H:%M").to_string();
        let created_label = self.session.created_at.format("%d/%m/%Y-%H:%M").to_string();

        if let Some(branch) = self.branch() {
            write!(
                f,
                "{agent_name} {} {} {} {} {}",
                path_label.cyan().bold(),
                branch.white(),
                session_name.dimmed().bold(),
                updated_label.blue(),
                created_label.blue(),
            )
        } else {
            write!(
                f,
                "{agent_name} {} {} {} {}",
                path_label.cyan().bold(),
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
    updated_at: chrono::DateTime<chrono::Utc>,
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
        Err::<(), std::io::Error>(cmd.exec())
            .context("failed to exec agent CLI")
            .attach_with(|| format!("agent={}", session.agent.name()))
            .attach_with(|| format!("workspace={}", session.workspace.display()))
            .attach_with(|| format!("session_id={}", session.id))?;

        Ok(())
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
    use chrono::DateTime;
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_search_corpus_matches_ags_visible_plus_hidden_filtering() {
        let display = "cx  ~/repo   branch   session name  09/05/2026-10:00";
        let hidden = "first user prompt\nassistant reply";

        let search = search_corpus(display, hidden);

        assert_that!(
            search,
            eq("cx ~/repo branch session name 09/05/2026-10:00 first user prompt assistant reply")
        );
    }

    #[test]
    fn test_json_session_renders_plain_ags_summary_and_resume_command() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("repo");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let created_at = DateTime::from_timestamp(1_700_000_000, 0).expect("test timestamp should be valid");
        let updated_at = DateTime::from_timestamp(1_700_000_100, 0).expect("test timestamp should be valid");
        let session = Session {
            id: "session-id".to_string(),
            agent: Agent::Codex,
            name: "fix issue".to_string(),
            search_text: "hidden prompt".to_string(),
            workspace: workspace.clone(),
            path: dir.path().join("session.jsonl"),
            created_at: created_at.to_utc(),
            updated_at: updated_at.to_utc(),
        };
        let renderable = RenderableSession::from(session);

        assert_that!(
            JsonSession::new(&renderable, dir.path()),
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
                result_of!(|row: &JsonSession| row.updated_at, eq(updated_at.to_utc())),
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
