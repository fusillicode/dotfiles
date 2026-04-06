use std::fmt::Display;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use agm_core::agent::Agent;
use agm_core::agent::session::Session;
use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt as _;
use strum::EnumIter;
use strum::IntoEnumIterator as _;

pub fn run() -> rootcause::Result<()> {
    let mut sessions = Vec::new();

    sessions.extend(agm_core::agent::session_loader::claude::load_sessions()?);
    sessions.extend(agm_core::agent::session_loader::codex::load_sessions()?);
    sessions.extend(agm_core::agent::session_loader::cursor::load_sessions()?);

    sessions.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    let Some(selected) = ytil_tui::minimal_multi_select(sessions.into_iter().map(RenderableSession).collect())? else {
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
            Agent::Claude => pad_right("CLAUDE", 6).red().bold().to_string(),
            Agent::Codex => pad_right("CODEX", 6).green().bold().to_string(),
            Agent::Cursor => pad_right("CURSOR", 6).bright_black().bold().to_string(),
            Agent::Gemini | Agent::Opencode => pad_right(&self.0.agent.to_string(), 6),
        };

        let session_name = display_session_name(&self.0.name, 42);

        let updated_label = pad_right(&self.0.updated_at.format("%d-%m-%Y %H:%M").to_string(), 16);
        let created_label = pad_right(&self.0.created_at.format("%d-%m-%Y %H:%M").to_string(), 16);

        write!(
            f,
            "{agent_name} {} {} {} {}",
            session_name.white().bold(),
            updated_label.dimmed(),
            created_label.dimmed(),
            render_workspace_path(&self.0.workspace).blue(),
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

fn display_session_name(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = normalized.chars().collect();
    let (out, chars_count) = if chars.len() <= max_chars {
        (normalized, max_chars)
    } else {
        let mut trimmed: String = chars.into_iter().take(max_chars).collect();
        trimmed.push('…');
        (trimmed, max_chars.saturating_add(1))
    };
    pad_right(&out, chars_count)
}

fn pad_right(value: &str, width: usize) -> String {
    format!("{value:<width$}")
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

fn render_workspace_path(path: &Path) -> String {
    std::env::var_os("HOME").map(PathBuf::from).as_deref().map_or_else(
        || agm_core::short_path(path, Path::new("/")),
        |home| agm_core::short_path(path, home),
    )
}
