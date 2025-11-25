use std::fmt::Display;
use std::ops::Deref;

use color_eyre::owo_colors::OwoColorize as _;
use ytil_git::branch::Branch;

/// A wrapper around [`Branch`] for display purposes.
///
/// # Rationale
/// Provides a custom [`Display`] implementation for branch selection UI.
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
        write!(f, "{} {}", self.name(), styled_date_time.blue())
    }
}

/// Prompts the user to select a branch from all available branches.
///
/// # Returns
/// The selected [`Branch`] or [`None`] if no selection was made.
///
/// # Errors
/// - If [`ytil_git::branch::get_all_no_redundant`] fails.
/// - If [`crate::minimal_select`] fails.
pub fn select() -> color_eyre::Result<Option<Branch>> {
    let branches = ytil_git::branch::get_all_no_redundant()?;

    let Some(branch) = crate::minimal_select(branches.into_iter().map(RenderableBranch).collect())? else {
        return Ok(None);
    };

    Ok(Some(branch.0))
}
