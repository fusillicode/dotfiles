use std::collections::HashSet;
use std::process::Command;

use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use ytil_cmd::CmdExt as _;

const PATH_LINE_PREFIX: &str = "diff --git ";

/// Retrieves the current `git diff` output as a [`Vec<String>`].
///
/// # Returns
/// A [`Vec<String>`] where each line corresponds to a line of `git diff` output.
///
/// # Errors
/// - If the `git diff` command fails to execute or returns a non-zero exit code.
/// - If extracting the output from the command fails.
pub fn get() -> color_eyre::Result<Vec<String>> {
    let output = Command::new("git").arg("diff").exec()?;

    Ok(ytil_cmd::extract_success_output(&output)?
        .lines()
        .map(str::to_string)
        .collect())
}

/// Extracts file paths and their modified line numbers from `git diff` output.
///
/// # Arguments
/// - `diff_output` The lines of `git diff` output to parse.
///
/// # Returns
/// A [`HashSet`] mapping file paths to vectors of line numbers where changes occurred.
///
/// # Errors
/// - Missing path delimiter in the diff line.
/// - Unable to extract the filepath from the diff line.
/// - Unable to access subsequent lines for line numbers.
/// - Missing comma delimiter in the hunk header.
/// - Unable to extract the line number from the hunk header.
/// - Line number cannot be parsed as a valid [`usize`].
///
/// # Assumptions
/// Assumes `diff_output` is in standard unified diff format produced by `git diff`.
pub fn get_paths_with_lnums(diff_output: &[String]) -> color_eyre::Result<HashSet<(&str, usize)>> {
    let mut out = HashSet::new();

    for (diff_line_idx, diff_line) in diff_output.iter().enumerate() {
        let Some(path_line) = diff_line.strip_prefix(PATH_LINE_PREFIX) else {
            continue;
        };

        let path_idx = path_line
            .find(" b/")
            .ok_or_else(|| {
                eyre!(
                    "error missing path prefix in path_line | path_line={path_line:?} diff_line_idx={diff_line_idx} diff_line={diff_line:?}"
                )
            })?
            .saturating_add(3);

        let path = path_line.get(path_idx..).ok_or_else(|| {
            eyre!("error extracting path from path_line | path_idx={path_idx} path_line={path_line:?} diff_line_idx={diff_line_idx} diff_line={diff_line:?}")
        })?;

        let lnum_lines_start_idx = diff_line_idx.saturating_add(1);
        let maybe_lnum_lines = diff_output
            .get(lnum_lines_start_idx..)
            .ok_or_else(|| eyre!("error extracting lnum_lines from diff_output | lnum_lines_start_idx={lnum_lines_start_idx} diff_line_idx={diff_line_idx}"))?;

        for maybe_lnum_line in maybe_lnum_lines {
            if maybe_lnum_line.starts_with(PATH_LINE_PREFIX) {
                break;
            }
            let Some(lnum_line) = maybe_lnum_line.strip_prefix("@@ ") else {
                continue;
            };

            // Adjusting `git diff` lnum to match what is displayed by Neovim.
            let lnum = extract_lnum(lnum_line)?.saturating_add(3);

            out.insert((path, lnum));
        }
    }

    Ok(out)
}

/// Extracts the line number from a `git diff` hunk header line.
///
/// # Arguments
/// - `lnum_line` The hunk header line (e.g., "@@ -42,7 +42,7 @@").
///
/// # Returns
/// The line number as a `usize` (e.g., 42).
///
/// # Errors
/// - If the line number cannot be extracted between " +" and ",".
/// - If the extracted value cannot be parsed as a valid [`usize`].
fn extract_lnum(lnum_line: &str) -> color_eyre::Result<usize> {
    let lnum_value = find_between(lnum_line, " +", ",")
        .ok_or_else(|| eyre!("error extracting lnum from lnum_line | lnum_line={lnum_line:?}"))?;
    lnum_value
        .parse()
        .wrap_err_with(|| eyre!("error parsing lnum value as usize | lnum_value={lnum_value:?}"))
}

/// Extracts the substring between the first occurrence of `start` and the first `end` after it.
///
/// # Arguments
/// - `haystack` The string to search in.
/// - `start` The starting delimiter.
/// - `end` The ending delimiter.
///
/// # Returns
/// The substring between `start` and `end`, or `None` if either delimiter is not found or `end` precedes `start`.
fn find_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_idx = haystack.find(start)?.saturating_add(start.len());
    let rest = haystack.get(start_idx..)?;
    let end_idx = rest.find(end)?;
    rest.get(..end_idx)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::single_file_single_hunk(
        vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "index 1234567..abcdef0 100644".to_string(),
            "--- a/src/main.rs".to_string(),
            "+++ b/src/main.rs".to_string(),
            "@@ -42,7 +42,7 @@".to_string(),
        ],
        HashSet::from([("src/main.rs", 45)])
    )]
    #[case::multiple_files(
        vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "index 1234567..abcdef0 100644".to_string(),
            "--- a/src/main.rs".to_string(),
            "+++ b/src/main.rs".to_string(),
            "@@ -10,5 +10,5 @@".to_string(),
            "diff --git a/src/lib.rs b/src/lib.rs".to_string(),
            "index fedcba9..7654321 100644".to_string(),
            "--- a/src/lib.rs".to_string(),
            "+++ b/src/lib.rs".to_string(),
            "@@ -20,3 +20,3 @@".to_string(),
        ],
        HashSet::from([("src/main.rs", 13), ("src/lib.rs", 23)])
    )]
    #[case::multiple_hunks_same_file(
        vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "index 1234567..abcdef0 100644".to_string(),
            "--- a/src/main.rs".to_string(),
            "+++ b/src/main.rs".to_string(),
            "@@ -10,5 +10,5 @@".to_string(),
            "@@ -50,2 +50,2 @@".to_string(),
        ],
        HashSet::from([("src/main.rs", 13), ("src/main.rs", 53)])
    )]
    #[case::empty_input(vec![], HashSet::new())]
    #[case::no_hunks(
        vec!["diff --git a/src/main.rs b/src/main.rs".to_string()],
        HashSet::new()
    )]
    #[case::non_diff_lines_ignored(
        vec![
            "index 123..456 789".to_string(),
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "index 1234567..abcdef0 100644".to_string(),
            "--- a/src/main.rs".to_string(),
            "+++ b/src/main.rs".to_string(),
            "@@ -42,7 +42,7 @@".to_string(),
        ],
        HashSet::from([("src/main.rs", 45)])
    )]
    #[case::multiple_files_with_multiple_hunks(
        vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "index 1234567..abcdef0 100644".to_string(),
            "--- a/src/main.rs".to_string(),
            "+++ b/src/main.rs".to_string(),
            "@@ -10,5 +10,5 @@".to_string(),
            "@@ -50,2 +50,2 @@".to_string(),
            "diff --git a/src/lib.rs b/src/lib.rs".to_string(),
            "index fedcba9..7654321 100644".to_string(),
            "--- a/src/lib.rs".to_string(),
            "+++ b/src/lib.rs".to_string(),
            "@@ -20,3 +20,3 @@".to_string(),
            "@@ -60,1 +60,1 @@".to_string(),
        ],
        HashSet::from([("src/main.rs", 13), ("src/main.rs", 53), ("src/lib.rs", 23), ("src/lib.rs", 63)])
    )]
    fn test_get_paths_with_lnums_success(#[case] input: Vec<String>, #[case] expected: HashSet<(&str, usize)>) {
        assert2::let_assert!(Ok(result) = get_paths_with_lnums(&input));
        pretty_assertions::assert_eq!(result, expected);
    }

    #[rstest]
    #[case::missing_b_delimiter(
        vec!["diff --git a/src/main.rs".to_string()],
        "error missing path prefix"
    )]
    #[case::missing_comma_in_hunk(
        vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "@@ -42 +42 @@".to_string(),
        ],
        "error extracting lnum"
    )]
    #[case::invalid_lnum(
        vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "@@ -abc,5 +abc,5 @@".to_string(),
        ],
        "error parsing lnum value"
    )]
    fn test_get_paths_with_lnums_error(#[case] input: Vec<String>, #[case] expected_error_contains: &str) {
        assert2::let_assert!(Err(err) = get_paths_with_lnums(&input));
        assert!(err.to_string().contains(expected_error_contains));
    }

    #[test]
    fn extract_lnum_when_valid_lnum_line_returns_correct_usize() {
        let input = "@@ -42,7 +42,7 @@";
        assert2::let_assert!(Ok(result) = extract_lnum(input));
        pretty_assertions::assert_eq!(result, 42);
    }

    #[rstest]
    #[case::missing_plus_prefix("@@ -42,7 42,7 @@", "error extracting lnum")]
    #[case::missing_comma_suffix("@@ -42,7 +42 7 @@", "error extracting lnum")]
    #[case::lnum_value_not_numeric("@@ -42,7 +abc,7 @@", "error parsing lnum value")]
    fn extract_lnum_error_cases(#[case] input: &str, #[case] expected_error_contains: &str) {
        assert2::let_assert!(Err(err) = extract_lnum(input));
        assert!(err.to_string().contains(expected_error_contains));
    }

    #[rstest]
    #[case::normal_case("prefixSTARTcontentENDsuffix", "START", "END", Some("content"))]
    #[case::empty_between("STARTEND", "START", "END", Some(""))]
    #[case::start_at_beginning("STARTcontentEND", "START", "END", Some("content"))]
    #[case::end_at_end("prefixSTARTcontentEND", "START", "END", Some("content"))]
    #[case::no_start("no start here", "START", "END", None)]
    #[case::start_no_end("STARTcontent no end", "START", "END", None)]
    #[case::end_before_start("ENDbeforeSTART", "START", "END", None)]
    #[case::multiple_occurrences("STARTfirstEND STARTsecondEND", "START", "END", Some("first"))]
    fn find_between_cases(
        #[case] haystack: &str,
        #[case] start: &str,
        #[case] end: &str,
        #[case] expected: Option<&str>,
    ) {
        let result = find_between(haystack, start, end);
        pretty_assertions::assert_eq!(result, expected);
    }
}
