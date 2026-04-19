use std::fmt::Display;
use std::ops::Deref;
use std::path::Path;

use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt as _;
use ytil_git::branch::Branch;

/// Prompts the user to select a branch from all available branches.
///
/// The previously checked-out branch (`@{-1}`) is placed first when available.
/// Then up to seven branches whose latest commit was made with the configured
/// `user.email` are shown before the remaining recency-ordered list.
///
/// # Errors
/// - If repository discovery fails.
/// - If [`ytil_git::branch::get_all_no_redundant`] fails.
/// - If [`crate::minimal_select`] fails.
pub fn select() -> rootcause::Result<Option<Branch>> {
    let repo = ytil_git::repo::discover(Path::new(".")).context("error discovering repo for branch selection")?;
    let branches = prioritize_branches(
        ytil_git::branch::get_all_no_redundant(&repo)?,
        ytil_git::branch::get_previous(&repo).as_deref(),
        ytil_git::branch::get_user_email(&repo)?.as_deref(),
    );

    let Some(branch) = crate::minimal_select(branches.into_iter().map(RenderableBranch).collect())? else {
        return Ok(None);
    };

    Ok(Some(branch.0))
}

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
            styled_date_time.green(),
            styled_email.blue().bold(),
        )
    }
}

fn prioritize_branches(branches: Vec<Branch>, previous_branch: Option<&str>, user_email: Option<&str>) -> Vec<Branch> {
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

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use chrono::Utc;
    use rstest::rstest;
    use ytil_git::branch::Branch;

    use super::prioritize_branches;

    #[rstest]
    #[case(
        vec![
            branch("main", "other@example.com", 30),
            branch("feature-a", "me@example.com", 20),
            branch("feature-b", "me@example.com", 10),
        ],
        Some("feature-b"),
        Some("me@example.com"),
        vec![
            branch("feature-b", "me@example.com", 10),
            branch("feature-a", "me@example.com", 20),
            branch("main", "other@example.com", 30),
        ]
    )]
    #[case(
        vec![
            remote_branch("origin/feature-a", "me@example.com", 30),
            branch("main", "other@example.com", 20),
        ],
        Some("feature-a"),
        Some("me@example.com"),
        vec![
            remote_branch("origin/feature-a", "me@example.com", 30),
            branch("main", "other@example.com", 20),
        ]
    )]
    #[case(
        vec![
            branch("main", "other@example.com", 30),
            branch("feature-a", "me@example.com", 20),
            branch("feature-b", "me@example.com", 10),
        ],
        Some("feature-a"),
        Some("me@example.com"),
        vec![
            branch("feature-a", "me@example.com", 20),
            branch("feature-b", "me@example.com", 10),
            branch("main", "other@example.com", 30),
        ]
    )]
    fn prioritize_branches_prioritizes_previous_branch_cases(
        #[case] branches: Vec<Branch>,
        #[case] previous_branch: Option<&str>,
        #[case] user_email: Option<&str>,
        #[case] expected: Vec<Branch>,
    ) {
        let ordered = prioritize_branches(branches, previous_branch, user_email);

        pretty_assertions::assert_eq!(ordered, expected);
    }

    #[test]
    fn test_prioritize_branches_puts_only_the_wanted_number_of_branches_matching_email_before_rest() {
        let branches = vec![
            branch("other-1", "other@example.com", 100),
            branch("mine-1", "me@example.com", 99),
            branch("mine-2", "me@example.com", 98),
            branch("mine-3", "me@example.com", 97),
            branch("mine-4", "me@example.com", 96),
            branch("mine-5", "me@example.com", 95),
            branch("mine-6", "me@example.com", 94),
            branch("mine-7", "me@example.com", 93),
            branch("mine-8", "me@example.com", 92),
            branch("other-2", "other@example.com", 91),
        ];

        let ordered = prioritize_branches(branches, None, Some("me@example.com"));

        pretty_assertions::assert_eq!(
            &ordered,
            &[
                branch("mine-1", "me@example.com", 99),
                branch("mine-2", "me@example.com", 98),
                branch("mine-3", "me@example.com", 97),
                branch("mine-4", "me@example.com", 96),
                branch("mine-5", "me@example.com", 95),
                branch("other-1", "other@example.com", 100),
                branch("mine-6", "me@example.com", 94),
                branch("mine-7", "me@example.com", 93),
                branch("mine-8", "me@example.com", 92),
                branch("other-2", "other@example.com", 91),
            ],
        );
    }

    #[rstest]
    #[case(
        vec![
            branch("other-1", "other@example.com", 100),
            branch("mine-1", "me@example.com", 99),
            branch("other-2", "other@example.com", 98),
            branch("mine-2", "me@example.com", 97),
        ],
        None,
        Some("me@example.com"),
        vec![
            branch("mine-1", "me@example.com", 99),
            branch("mine-2", "me@example.com", 97),
            branch("other-1", "other@example.com", 100),
            branch("other-2", "other@example.com", 98),
        ]
    )]
    #[case(
        vec![
            branch("main", "other@example.com", 30),
            branch("feature-a", "me@example.com", 20),
            branch("feature-b", "me@example.com", 10),
        ],
        Some("feature-b"),
        None,
        vec![
            branch("feature-b", "me@example.com", 10),
            branch("main", "other@example.com", 30),
            branch("feature-a", "me@example.com", 20),
        ]
    )]
    #[case(
        vec![
            branch("main", "other@example.com", 30),
            branch("feature-a", "other@example.com", 20),
        ],
        Some("missing"),
        None,
        vec![
            branch("main", "other@example.com", 30),
            branch("feature-a", "other@example.com", 20),
        ]
    )]
    #[case(
        vec![
            branch("main", "other@example.com", 30),
            branch("feature-a", "other@example.com", 20),
        ],
        None,
        None,
        vec![
            branch("main", "other@example.com", 30),
            branch("feature-a", "other@example.com", 20),
        ]
    )]
    fn prioritize_branches_misc_cases(
        #[case] branches: Vec<Branch>,
        #[case] previous_branch: Option<&str>,
        #[case] user_email: Option<&str>,
        #[case] expected: Vec<Branch>,
    ) {
        let ordered = prioritize_branches(branches, previous_branch, user_email);

        pretty_assertions::assert_eq!(ordered, expected);
    }

    fn branch(name: &str, email: &str, timestamp: i64) -> Branch {
        Branch::Local {
            name: name.to_string(),
            committer_email: email.to_string(),
            committer_date_time: DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap(),
        }
    }

    fn remote_branch(name: &str, email: &str, timestamp: i64) -> Branch {
        Branch::Remote {
            name: name.to_string(),
            committer_email: email.to_string(),
            committer_date_time: DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap(),
        }
    }
}
