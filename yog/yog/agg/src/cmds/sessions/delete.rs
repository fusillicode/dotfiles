use std::path::Path;

use owo_colors::OwoColorize;
use rootcause::report;
use ytil_agents::agent::session::SessionKey;
use ytil_agents::agent::session_deletion::DeletionOutcome;

use super::list::RenderableSession;

pub(super) fn delete_selected_sessions(selected: &[RenderableSession], home_dir: &Path) -> rootcause::Result<()> {
    let keys = selected
        .iter()
        .map(|session| SessionKey::new(session.session.agent, &session.session.id))
        .collect::<Vec<_>>();
    let report = ytil_agents::agent::session_deletion::delete_sessions(home_dir, &keys);
    let mut failures = Vec::new();

    for outcome in report.outcomes() {
        match outcome {
            DeletionOutcome::Deleted {
                key,
                related_session_count,
            } => render_deleted_session(key, *related_session_count, selected),
            DeletionOutcome::Failed { key, error } => failures.push(format!("{key}: {error}")),
        }
    }
    for skipped_path in report.skipped_paths() {
        println!(
            "{} {}",
            "Skipped unreadable session file".yellow().bold(),
            skipped_path.display()
        );
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(report!("session deletion failed").attach(failures.join("\n")))
    }
}

fn render_deleted_session(key: &SessionKey, related_session_count: usize, selected: &[RenderableSession]) {
    let Some(session) = selected
        .iter()
        .find(|session| session.session.agent == key.agent() && session.session.id == key.id())
    else {
        return;
    };
    println!("{} {session}", "Deleted".red().bold());
    if related_session_count > 0 {
        println!(
            "  └─ {} (related sessions: {related_session_count})",
            key.id().white().bold()
        );
    }
}
