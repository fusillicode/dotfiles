//! Issue creation command.

use owo_colors::OwoColorize;

/// Create a GitHub issue and develop it with an associated branch.
pub fn run() -> rootcause::Result<()> {
    let Some(issue_title) = ytil_tui::text_prompt("Issue title:")?.map(|x| x.trim().to_string()) else {
        return Ok(());
    };

    let Some(checkout_branch) = ytil_tui::yes_no_select("Checkout branch?")? else {
        return Ok(());
    };

    let created_issue = ytil_gh::issue::create(&issue_title)?;
    println!(
        "\n{} number={} title={issue_title:?}",
        "Issue created".green().bold(),
        created_issue.issue_nr
    );

    let develop_output = ytil_gh::issue::develop(&created_issue.issue_nr, checkout_branch)?;
    println!(
        "{} with name={:?}",
        "Branch created".green().bold(),
        develop_output.branch_name
    );

    Ok(())
}
