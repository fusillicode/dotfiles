use std::fmt::Display;
use std::ops::Deref;

use color_eyre::owo_colors::OwoColorize as _;
use ytil_git::branch::Branch;

struct RenderableBranch(Branch);

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

pub fn select() -> color_eyre::Result<Option<Branch>> {
    let branches = ytil_git::branch::get_all_no_redundant()?;

    let Some(branch) = crate::minimal_select(branches.into_iter().map(RenderableBranch).collect())? else {
        return Ok(None);
    };

    Ok(Some(branch.0))
}
