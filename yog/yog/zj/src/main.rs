//! Interactive Zellij session management.
//!
//! # Errors
//! - Zellij CLI invocation or user interaction fails.

use core::fmt::Display;

use owo_colors::OwoColorize;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_sys::cli::Args;

struct DisplaySession(ytil_zellij::Session);

impl Display for DisplaySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display)
    }
}

#[derive(EnumIter)]
enum Op {
    Attach,
    Kill,
    Delete,
}

impl Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attach => write!(f, "{}", "Attach".green().bold()),
            Self::Kill => write!(f, "{}", "Kill".yellow().bold()),
            Self::Delete => write!(f, "{}", "Delete".red().bold()),
        }
    }
}

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();
    if args.has_help() || args.first().is_some_and(|a| a == "help") {
        ytil_zellij::help()?;
        println!();
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let sessions: Vec<DisplaySession> = ytil_zellij::list_sessions()?.into_iter().map(DisplaySession).collect();
    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    let Some(selected) = ytil_tui::minimal_multi_select(sessions)? else {
        println!("\n\nNo sessions selected");
        return Ok(());
    };

    let Some(op) = ytil_tui::minimal_select::<Op>(Op::iter().collect())? else {
        println!("\n\nNo action selected");
        return Ok(());
    };

    match op {
        Op::Attach => {
            if selected.len() > 1 {
                println!("Cannot attach to multiple sessions, select only one.");
                return Ok(());
            }
            if let Some(session) = selected.first() {
                ytil_zellij::attach_session(&session.0.name)?;
            }
        }
        Op::Kill => {
            for session in &selected {
                ytil_zellij::kill_session(&session.0.name)?;
                println!("{} {}", "Killed".yellow().bold(), session.0.name);
            }
        }
        Op::Delete => {
            for session in &selected {
                ytil_zellij::delete_session(&session.0.name)?;
                println!("{} {}", "Deleted".red().bold(), session.0.name);
            }
        }
    }
    Ok(())
}
