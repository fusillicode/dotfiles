use std::fmt::Display;
use std::ops::Deref;
use std::path::Path;

use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt as _;
use ytil_git::branch::Branch;

/// A wrapper around [`Branch`] for display purposes.
struct RenderableBranch(pub Branch);

impl Deref for RenderableBranch {
    type Target = Branch;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for RenderableBranch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let styled_date_time = format!("({})", self.committer_date_time());
        let styled_email = format!("<{}>", self.committer_email());
        write!(
            f,
            "{} {} {}",
            self.name(),
            styled_email.dimmed(),
            styled_date_time.blue(),
        )
    }
}

/// Prompts the user to select a branch from all available branches.
///
/// The previously checked-out branch (`@{-1}`) is placed first when available.
///
/// # Errors
/// - If repository discovery fails.
/// - If [`ytil_git::branch::get_all_no_redundant`] fails.
/// - If [`crate::minimal_select`] fails.
pub fn select() -> rootcause::Result<Option<Branch>> {
    let repo = ytil_git::repo::discover(Path::new(".")).context("error discovering repo for branch selection")?;
    let mut branches = ytil_git::branch::get_all_no_redundant(&repo)?;

    if let Some(prev) = ytil_git::branch::get_previous(&repo)
        && let Some(idx) = branches.iter().position(|b| b.name_no_origin() == prev)
    {
        let branch = branches.remove(idx);
        branches.insert(0, branch);
    }

    let Some(branch) = crate::minimal_select(branches.into_iter().map(RenderableBranch).collect())? else {
        return Ok(None);
    };

    Ok(Some(branch.0))
}
