use std::process::Command;

use color_eyre::eyre::Context as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use convert_case::Case;
use convert_case::Casing as _;

/// Represents a newly created GitHub issue.
///
/// Contains the parsed details from the `gh issue create` command output.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct CreatedIssue {
    /// The title of the created issue.
    pub title: String,
    /// The repository URL prefix (e.g., `https://github.com/owner/repo/`).
    pub repo: String,
    /// The issue number (e.g., "123").
    pub issue_nr: String,
}

impl CreatedIssue {
    /// Creates a [`CreatedIssue`] from the `gh issue create` command output.
    ///
    /// Parses the output URL to extract repository and issue number.
    ///
    /// # Arguments
    /// - `title` The issue title.
    /// - `output` The stdout from `gh issue create`.
    ///
    /// # Returns
    /// The parsed [`CreatedIssue`].
    ///
    /// # Errors
    /// - Output does not contain "issues".
    /// - Repository or issue number parts are empty.
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

    /// Generates a branch title from the issue number and title.
    ///
    /// Formats as `{issue_nr}-{title}` where `title` is converted to kebab-case and leading/trailing dashes are
    /// trimmed.
    ///
    /// # Returns
    /// A string suitable for use as a Git branch name.
    pub fn branch_name(&self) -> String {
        format!(
            "{}-{}",
            self.issue_nr.trim_matches('-'),
            self.title.to_case(Case::Kebab).trim_matches('-')
        )
    }
}

/// Creates a new GitHub issue with the specified title.
///
/// This function invokes `gh issue create --title <title> --body ""` to create the issue.
///
/// # Arguments
/// - `title` The title of the issue to create.
///
/// # Returns
/// The [`CreatedIssue`] containing the parsed issue details.
///
/// # Errors
/// - If `title` is empty.
/// - Spawning or executing the `gh issue create` command fails.
/// - Command exits with non-zero status.
/// - Output cannot be parsed as a valid issue URL.
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
