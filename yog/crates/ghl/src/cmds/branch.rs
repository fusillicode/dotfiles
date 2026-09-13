//! Issue branch creation command.

use std::fmt::Display;
use std::fmt::Formatter;
use std::ops::Deref;

use owo_colors::OwoColorize;
use ytil_gh::issue::ListedIssue;

struct RenderableListedIssue(pub ListedIssue);

impl Deref for RenderableListedIssue {
    type Target = ListedIssue;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for RenderableListedIssue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            // The spacing before the title is required to align it with the first line.
            "{} {} {}",
            self.author.login.blue().bold(),
            self.updated_at.strftime("%d-%m-%Y %H:%M UTC"),
            self.title
        )
    }
}

/// Interactively create a GitHub branch from a selected issue.
pub fn run() -> rootcause::Result<()> {
    let issues = ytil_gh::issue::list()?;

    let Some(issue) = ytil_tui::minimal_select(issues.into_iter().map(RenderableListedIssue).collect())? else {
        return Ok(());
    };

    let Some(checkout_branch) = ytil_tui::yes_no_select("Checkout branch?")? else {
        return Ok(());
    };

    let develop_output = ytil_gh::issue::develop(&issue.number.to_string(), checkout_branch)?;
    println!(
        "{} with name={:?}",
        "Branch created".green().bold(),
        develop_output.branch_name
    );

    Ok(())
}
