//! Interactive Git branch picking and prioritization.

use std::fmt::Display;
use std::fmt::Formatter;
use std::ops::Deref;
use std::path::Path;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use ytil_git::branch::Branch;

pub fn run() -> rootcause::Result<()> {
    let Some(branch) = select_branch_with_current_first()? else {
        return Ok(());
    };

    println!("{}", branch.name_no_origin());
    Ok(())
}

fn select_branch_with_current_first() -> rootcause::Result<Option<Branch>> {
    let repo = ytil_git::repo::discover(Path::new(".")).context("error discovering repo for branch selection")?;
    let branches = prioritize_current_branch_first(
        ytil_git::branch::get_all_no_redundant(&repo)?,
        ytil_git::branch::get_current()?.as_str(),
        ytil_git::branch::get_previous(&repo).as_deref(),
        ytil_git::branch::get_user_email(&repo)?.as_deref(),
    );

    let Some(branch) = ytil_tui::minimal_select(branches.into_iter().map(RenderableBranch).collect())? else {
        return Ok(None);
    };

    Ok(Some(branch.0))
}

fn prioritize_current_branch_first(
    branches: Vec<Branch>,
    current_branch: &str,
    previous_branch: Option<&str>,
    user_email: Option<&str>,
) -> Vec<Branch> {
    let branches = prioritize_recent_branches(branches, previous_branch, user_email);
    let mut current = None;
    let mut rest = Vec::with_capacity(branches.len());

    for branch in branches {
        if current.is_none() && branch.name_no_origin() == current_branch {
            current = Some(branch);
        } else {
            rest.push(branch);
        }
    }

    current.into_iter().chain(rest).collect()
}

fn prioritize_recent_branches(
    branches: Vec<Branch>,
    previous_branch: Option<&str>,
    user_email: Option<&str>,
) -> Vec<Branch> {
    const MINE_DESIRED_COUNT: usize = 5;

    let branches_len = branches.len();
    let mut previous = None;
    let mut mine = Vec::new();
    let mut rest = Vec::new();

    for branch in branches {
        if previous.is_none() && previous_branch.is_some_and(|prev| branch.name_no_origin() == prev) {
            previous = Some(branch);
        } else if mine.len() < MINE_DESIRED_COUNT && user_email.is_some_and(|email| branch.committer_email() == email) {
            mine.push(branch);
        } else {
            rest.push(branch);
        }
    }

    let mut prioritized = Vec::with_capacity(branches_len);
    prioritized.extend(previous);
    prioritized.extend(mine);
    prioritized.extend(rest);
    prioritized
}

struct RenderableBranch(pub Branch);

impl Deref for RenderableBranch {
    type Target = Branch;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for RenderableBranch {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let styled_date_time = format!("({})", self.committer_date_time());
        let styled_email = format!("<{}>", self.committer_email());
        write!(
            f,
            "{} {} {}",
            self.name(),
            styled_date_time.green(),
            styled_email.blue().bold(),
        )
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case(
        vec![branch("main", 30), branch("feature-a", 20), branch("feature-b", 10)],
        "feature-b",
        vec![branch("feature-b", 10), branch("main", 30), branch("feature-a", 20)]
    )]
    #[case(
        vec![remote_branch("origin/feature-a", 30), branch("main", 20)],
        "feature-a",
        vec![remote_branch("origin/feature-a", 30), branch("main", 20)]
    )]
    #[case(
        vec![branch("main", 30), branch("feature-a", 20)],
        "missing",
        vec![branch("main", 30), branch("feature-a", 20)]
    )]
    fn test_prioritize_current_branch_first_when_current_branch_varies_orders_expected_branches(
        #[case] branches: Vec<Branch>,
        #[case] current_branch: &str,
        #[case] expected: Vec<Branch>,
    ) {
        assert_that!(
            prioritize_current_branch_first(branches, current_branch, None, None),
            eq(expected)
        );
    }

    #[test]
    fn test_prioritize_current_branch_first_preserves_gcu_recent_order_after_current() {
        let branches = vec![
            branch_with_email("other-1", "other@example.com", 100),
            branch_with_email("mine-1", "me@example.com", 99),
            branch_with_email("previous", "other@example.com", 98),
            branch_with_email("current", "me@example.com", 97),
            branch_with_email("mine-2", "me@example.com", 96),
        ];

        assert_that!(
            prioritize_current_branch_first(branches, "current", Some("previous"), Some("me@example.com")),
            eq(vec![
                branch_with_email("current", "me@example.com", 97),
                branch_with_email("previous", "other@example.com", 98),
                branch_with_email("mine-1", "me@example.com", 99),
                branch_with_email("mine-2", "me@example.com", 96),
                branch_with_email("other-1", "other@example.com", 100),
            ])
        );
    }

    fn branch(name: &str, timestamp: i64) -> Branch {
        branch_with_email(name, "me@example.com", timestamp)
    }

    fn branch_with_email(name: &str, email: &str, timestamp: i64) -> Branch {
        Branch::Local {
            name: name.to_string(),
            committer_email: email.to_string(),
            committer_date_time: Timestamp::from_second(timestamp).unwrap(),
        }
    }

    fn remote_branch(name: &str, timestamp: i64) -> Branch {
        Branch::Remote {
            name: name.to_string(),
            committer_email: "me@example.com".to_string(),
            committer_date_time: Timestamp::from_second(timestamp).unwrap(),
        }
    }
}
