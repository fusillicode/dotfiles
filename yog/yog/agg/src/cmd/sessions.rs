use std::fmt::Display;
use std::process::Command;
use std::process::Stdio;

use agg_core::agent::Agent;
use agg_core::agent::session::Session;
use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt as _;
use strum::EnumIter;
use strum::IntoEnumIterator as _;

pub fn run() -> rootcause::Result<()> {
    let mut sessions = Vec::new();

    sessions.extend(agg_core::agent::session_loader::claude::load_sessions()?);
    sessions.extend(agg_core::agent::session_loader::codex::load_sessions()?);
    sessions.extend(agg_core::agent::session_loader::cursor::load_sessions()?);

    sessions.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.created_at.cmp(&a.created_at))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    let renderable_sessions: Vec<RenderableSession> = sessions.into_iter().map(RenderableSession).collect();
    let Some(selected) = ytil_tui::minimal_multi_select(renderable_sessions, ToString::to_string, |session| {
        session.0.search_text.clone()
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

struct RenderableSession(Session);

impl Display for RenderableSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let agent_name = match self.0.agent {
            Agent::Claude => "CLAUDE".red().bold().to_string(),
            Agent::Codex => "CODEX".green().bold().to_string(),
            Agent::Cursor => "CURSOR".bright_black().bold().to_string(),
            Agent::Gemini | Agent::Opencode => self.0.agent.to_string(),
        };

        let path_label = agg_core::short_path(
            &self.0.workspace,
            std::env::var_os("HOME")
                .as_deref()
                .map_or_else(|| std::path::Path::new("/"), std::path::Path::new),
        );
        let session_name = ytil_tui::display_fixed_width(&self.0.name, 42);
        let updated_label = self.0.updated_at.format("%d/%m/%Y-%H:%M").to_string();
        let created_label = self.0.created_at.format("%d/%m/%Y-%H:%M").to_string();

        write!(
            f,
            "{agent_name} {} {} {} {}",
            path_label.blue(),
            session_name.white().bold(),
            updated_label.dimmed(),
            created_label.dimmed(),
        )
    }
}

#[derive(Debug, EnumIter)]
enum Op {
    Resume,
    Delete,
}

impl Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resume => write!(f, "{}", "Resume".green().bold()),
            Self::Delete => write!(f, "{}", "Delete".red().bold()),
        }
    }
}

fn launch_session(RenderableSession(session): &RenderableSession) -> rootcause::Result<()> {
    let (program, args) = session.build_resume_command()?;

    let mut cmd = Command::new(program);
    cmd.args(args);
    let status = cmd
        .current_dir(&session.workspace)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to launch agent CLI")
        .attach_with(|| format!("agent={}", session.agent.name()))
        .attach_with(|| format!("workspace={}", session.workspace.display()))
        .attach_with(|| format!("session_id={}", session.id))?;

    status
        .exit_ok()
        .context("agent CLI exited with non-zero status")
        .attach_with(|| format!("agent={}", session.agent.name()))
        .attach_with(|| format!("workspace={}", session.workspace.display()))
        .attach_with(|| format!("session_id={}", session.id))?;

    Ok(())
}

fn delete_session(session: &RenderableSession) -> rootcause::Result<()> {
    let delete_path = &session.0.path;
    if delete_path.is_dir() {
        std::fs::remove_dir_all(delete_path)
            .context("failed to delete session directory")
            .attach_with(|| format!("path={}", delete_path.display()))
            .attach_with(|| format!("session_id={}", session.0.id))?;
    } else {
        std::fs::remove_file(delete_path)
            .context("failed to delete session file")
            .attach_with(|| format!("path={}", delete_path.display()))
            .attach_with(|| format!("session_id={}", session.0.id))?;
    }
    println!("{} {session}", "Deleted".red().bold());
    Ok(())
}
