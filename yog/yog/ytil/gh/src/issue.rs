use std::process::Command;

use chrono::DateTime;
use chrono::Utc;
use color_eyre::eyre::Context as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use convert_case::Case;
use convert_case::Casing as _;
use serde::Deserialize;
use ytil_cmd::CmdExt;

/// Represents a newly created GitHub issue.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct CreatedIssue {
    pub title: String,
    pub repo: String,
    pub issue_nr: String,
}

impl CreatedIssue {
    /// Creates a [`CreatedIssue`] from the `gh issue create` command output.
    ///
    /// # Errors
    /// - Output parsing fails.
    fn new(title: &str, output: &str) -> color_eyre::Result<Self> {
        let get_not_empty_field = |maybe_value: Option<&str>, field: &str| -> color_eyre::Result<String> {
            maybe_value
                .ok_or_else(|| eyre!("error building CreateIssueOutput | missing={field:?} output={output:?}"))
                .and_then(|s| {
                    if s.is_empty() {
                        Err(eyre!(
                            "error building CreateIssueOutput | empty={field:?} output={output:?}"
                        ))
                    } else {
                        Ok(s.trim_matches('/').to_string())
                    }
                })
        };

        let mut split = output.split("issues");

        Ok(Self {
            title: title.to_string(),
            repo: get_not_empty_field(split.next(), "repo")?,
            issue_nr: get_not_empty_field(split.next(), "issue_nr")?,
        })
    }

    /// Generates a branch name from the issue number and title.
    pub fn branch_name(&self) -> String {
        format!(
            "{}-{}",
            self.issue_nr.trim_matches('-'),
            self.title.to_case(Case::Kebab).trim_matches('-')
        )
    }
}

/// Output of [`develop`] a GitHub issue.
pub struct DevelopOutput {
    pub branch_ref: String,
    pub branch_name: String,
}

#[derive(Debug, Deserialize)]
pub struct ListedIssue {
    pub author: Author,
    pub title: String,
    pub number: usize,
    #[serde(rename = "updatedAt")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct Author {
    pub login: String,
}

/// Creates a new GitHub issue with the specified title.
///
/// # Errors
/// - Title is empty or `gh issue create` fails.
pub fn create(title: &str) -> color_eyre::Result<CreatedIssue> {
    if title.is_empty() {
        bail!("cannot create GitHub issue with empty title")
    }

    let output = Command::new("gh")
        .args(["issue", "create", "--title", title, "--body", ""])
        .output()
        .wrap_err_with(|| eyre!("error creating GitHub issue | title={title:?}"))?;

    let created_issue = ytil_cmd::extract_success_output(&output)
        .and_then(|output| CreatedIssue::new(title, &output))
        .wrap_err_with(|| eyre!("error parsing created issue output | title={title:?}"))?;

    Ok(created_issue)
}

/// Creates a branch for the supplied GitHub issue number.
///
/// # Errors
/// - `gh issue develop` fails or output parsing fails.
pub fn develop(issue_number: &str, checkout: bool) -> color_eyre::Result<DevelopOutput> {
    let mut args = vec!["issue", "develop", issue_number];

    if checkout {
        args.push("-c");
    }

    let output = Command::new("gh")
        .args(args)
        .exec()
        .wrap_err_with(|| eyre!("error develop GitHub issue | issue_number={issue_number}"))?;

    let branch_ref = str::from_utf8(&output.stdout)?.trim().to_string();
    let branch_name = branch_ref
        .rsplit('/')
        .next()
        .ok_or_else(|| eyre!("error extracting branch name from develop output | output={branch_ref:?}"))?
        .to_string();

    Ok(DevelopOutput {
        branch_ref,
        branch_name,
    })
}

/// Lists all GitHub issues for the current repository.
///
/// # Errors
/// - `gh issue list` fails or JSON deserialization fails.
pub fn list() -> color_eyre::Result<Vec<ListedIssue>> {
    let output = Command::new("gh")
        .args(["issue", "list", "--json", "number,title,author,updatedAt"])
        .exec()
        .wrap_err_with(|| eyre!("error listing GitHub issues"))?;

    let list_output = str::from_utf8(&output.stdout)?.trim().to_string();

    Ok(serde_json::from_str(&list_output)?)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn created_issue_new_parses_valid_output() {
        assert2::let_assert!(Ok(actual) = CreatedIssue::new("Test Issue", "https://github.com/owner/repo/issues/123"));
        pretty_assertions::assert_eq!(
            actual,
            CreatedIssue {
                title: "Test Issue".to_string(),
                repo: "https://github.com/owner/repo".to_string(),
                issue_nr: "123".to_string(),
            }
        );
    }

    #[rstest]
    #[case("", "error building CreateIssueOutput | empty=\"repo\" output=\"\"")]
    #[case("issues", "error building CreateIssueOutput | empty=\"repo\" output=\"issues\"")]
    #[case(
        "https://github.com/owner/repo/123",
        "error building CreateIssueOutput | missing=\"issue_nr\" output=\"https://github.com/owner/repo/123\""
    )]
    #[case(
        "repo/issues",
        "error building CreateIssueOutput | empty=\"issue_nr\" output=\"repo/issues\""
    )]
    fn created_issue_new_errors_on_invalid_output(#[case] output: &str, #[case] expected_error: &str) {
        assert2::let_assert!(Err(err) = CreatedIssue::new("title", output));
        pretty_assertions::assert_eq!(err.to_string(), expected_error);
    }

    #[rstest]
    #[case("Fix bug", "42", "42-fix-bug")]
    #[case("-Fix bug", "-42-", "42-fix-bug")]
    fn created_issue_branch_name_formats_correctly(
        #[case] title: &str,
        #[case] issue_nr: &str,
        #[case] expected: &str,
    ) {
        let issue = CreatedIssue {
            title: title.to_string(),
            issue_nr: issue_nr.to_string(),
            repo: "https://github.com/owner/repo/".to_string(),
        };
        pretty_assertions::assert_eq!(issue.branch_name(), expected);
    }
}
