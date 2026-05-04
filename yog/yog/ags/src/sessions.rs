use std::cell::RefCell;
use std::fmt::Display;
use std::fmt::Formatter;
use std::process::Command;
use std::process::Stdio;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_agents::agent::Agent;
use ytil_agents::agent::session::Session;

pub fn run() -> rootcause::Result<()> {
    let mut sessions = Vec::new();

    sessions.extend(ytil_agents::agent::session_loader::load_sessions()?);

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
