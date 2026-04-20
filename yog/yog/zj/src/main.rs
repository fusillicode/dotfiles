//! Interactive Zellij session management.
//!
//! # Errors
//! - Zellij CLI invocation or user interaction fails.

use std::fmt::Display;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_sys::cli::Args;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();

    let is_zj_help = (args.len() == 1 && args.has_help()) || args.first().is_some_and(|a| a == "help");
    if is_zj_help {
        ytil_zellij::help()?;
        println!();
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    if !args.is_empty() {
        return ytil_zellij::forward(&args);
    }

    let sessions: Vec<DisplaySession> = ytil_zellij::list_sessions()?.into_iter().map(DisplaySession).collect();
    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    let Some(selected) = ytil_tui::minimal_multi_select(sessions, ToString::to_string, ToString::to_string)? else {
        println!("\n\nNo sessions selected");
        return Ok(());
    };

    let Some(op) = ytil_tui::minimal_select::<Op>(Op::iter().collect())? else {
        println!("\n\nNo action selected");
        return Ok(());
    };

    match op {
        Op::Attach => {
            let session = ytil_tui::require_single(&selected, "sessions")?;
            ytil_zellij::attach_session(&session.0.name)
                .attach(format!("op={op:?}"))
                .attach(format!("session={}", session.0.name))?;
        }
        Op::Restart => {
            let session = ytil_tui::require_single(&selected, "sessions")?;
            let name = &session.0.name;
            ytil_zellij::kill_session(name)
                .attach(format!("op={op:?}"))
                .attach(format!("session={name}"))?;
            ytil_zellij::attach_session(name)
                .attach(format!("op={op:?}"))
                .attach(format!("session={name}"))?;
        }
        Op::Kill => {
            for session in &selected {
                ytil_zellij::kill_session(&session.0.name)
                    .attach(format!("op={op:?}"))
                    .attach(format!("session={}", session.0.name))?;
                println!("{} {}", "Killed".yellow().bold(), session.0.name);
            }
        }
        Op::Delete => {
            for session in &selected {
                ytil_zellij::delete_session(&session.0.name)
                    .attach(format!("op={op:?}"))
                    .attach(format!("session={}", session.0.name))?;
                println!("{} {}", "Deleted".red().bold(), session.0.name);
            }
        }
    }

    Ok(())
}

struct DisplaySession(ytil_zellij::Session);

impl Display for DisplaySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display)
    }
}

#[derive(Debug, EnumIter)]
enum Op {
    Attach,
    Restart,
    Kill,
    Delete,
}

impl Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attach => write!(f, "{}", "Attach".green().bold()),
            Self::Restart => write!(f, "{}", "Restart".cyan().bold()),
            Self::Kill => write!(f, "{}", "Kill".yellow().bold()),
            Self::Delete => write!(f, "{}", "Delete".red().bold()),
        }
    }
}
