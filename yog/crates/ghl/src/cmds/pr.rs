//! Pull request creation command.

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;

/// Prompt for a branch and create a pull request for it.
pub fn run() -> rootcause::Result<()> {
    let Some(branch) = ytil_tui::git_branch::select()? else {
        return Ok(());
    };

    let title = pr_title_from_branch_name(branch.name_no_origin())?;
    let pr_url = ytil_gh::pr::create(&title)?;
    println!("{} title={title:?} pr_url={pr_url:?}", "PR created".green().bold());

    Ok(())
}

/// Parses a branch name to generate a pull request title.
fn pr_title_from_branch_name(branch_name: &str) -> rootcause::Result<String> {
    let mut parts = branch_name.split('-');

    let x = parts
        .next()
        .ok_or_else(|| report!("error malformed branch_name"))
        .attach_with(|| format!("branch_name={branch_name:?}"))?;
    let issue_number: usize = x
        .parse()
        .context("error parsing issue number")
        .attach_with(|| format!("branch_name={branch_name:?} issue_number={x:?}"))?;

    let mut title = String::with_capacity(branch_name.len());
    for (i, word) in parts.enumerate() {
        if i > 0 {
            title.push(' ');
        }
        if i == 0 {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                for c in first.to_uppercase() {
                    title.push(c);
                }
                title.push_str(chars.as_str());
            }
        } else {
            title.push_str(word);
        }
    }

    if title.is_empty() {
        Err(report!("error empty title")).attach_with(|| format!("branch_name={branch_name:?}"))?;
    }

    Ok(format!("[{issue_number}]: {title}"))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case("43-foo-bar-baz", "[43]: Foo bar baz")]
    #[case("1-hello", "[1]: Hello")]
    #[case("123-long-branch-name-here", "[123]: Long branch name here")]
    fn test_pr_title_from_branch_name_when_valid_input_formats_correctly(#[case] input: &str, #[case] expected: &str) {
        assert_that!(pr_title_from_branch_name(input).unwrap(), eq(expected));
    }

    #[rstest]
    #[case("abc-foo", "error parsing issue number")]
    #[case("42", "error empty title")]
    #[case("", "error parsing issue number")]
    fn test_pr_title_from_branch_name_when_invalid_input_returns_error(
        #[case] input: &str,
        #[case] expected_ctx: &str,
    ) {
        assert_that!(
            pr_title_from_branch_name(input),
            err(result_of!(
                |err: &rootcause::Report| err.format_current_context().to_string(),
                eq(expected_ctx)
            ))
        );
    }
}
