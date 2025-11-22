use std::path::Path;
use std::process::Command;

use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use ytil_cmd::CmdExt as _;

const PATH_LINE_PREFIX: &str = "diff --git ";

/// Retrieves the current `git diff` raw output with `-U0` for fine-grained diffs as a [`Vec<String>`].
///
/// # Returns
/// A [`Vec<String>`] where each line corresponds to a line of `git diff` raw output.
///
/// # Errors
/// - If the `git diff` command fails to execute or returns a non-zero exit code.
/// - If extracting the output from the command fails.
///
/// # Rationale
/// Uses `-U0` to produce the most fine-grained line diffs.
pub fn get_raw(path: Option<&Path>) -> color_eyre::Result<Vec<String>> {
    let mut args = vec!["diff".into(), "-U0".into()];

    if let Some(path) = path {
        args.push(path.display().to_string());
    }

    let output = Command::new("git").args(args).exec()?;

    Ok(ytil_cmd::extract_success_output(&output)?
        .lines()
        .map(str::to_string)
        .collect())
}

/// Extracts file paths and starting line numbers of hunks from `git diff` output.
///
/// # Arguments
/// - `raw_diff_output` The lines of `git diff` output to parse.
///
/// # Returns
/// A [`Vec<(&str, usize)>`] where each tuple contains a filepath and the starting line number of a hunk.
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
/// Assumes `raw_diff_output` is in standard unified diff format produced by `git diff`.
pub fn get_hunks(raw_diff_output: &[String]) -> color_eyre::Result<Vec<(&str, usize)>> {
    let mut out = vec![];

    for (raw_diff_line_idx, raw_diff_line) in raw_diff_output.iter().enumerate() {
        let Some(path_line) = raw_diff_line.strip_prefix(PATH_LINE_PREFIX) else {
            continue;
        };

        let path_idx = path_line
            .find(" b/")
            .ok_or_else(|| {
                eyre!(
                    "error missing path prefix in path_line | path_line={path_line:?} raw_diff_line_idx={raw_diff_line_idx} raw_diff_line={raw_diff_line:?}"
                )
            })?
            .saturating_add(3);

        let path = path_line.get(path_idx..).ok_or_else(|| {
            eyre!("error extracting path from path_line | path_idx={path_idx} path_line={path_line:?} raw_diff_line_idx={raw_diff_line_idx} raw_diff_line={raw_diff_line:?}")
        })?;

        let lnum_lines_start_idx = raw_diff_line_idx.saturating_add(1);
        let maybe_lnum_lines = raw_diff_output
            .get(lnum_lines_start_idx..)
            .ok_or_else(|| eyre!("error extracting lnum_lines from raw_diff_output | lnum_lines_start_idx={lnum_lines_start_idx} raw_diff_line_idx={raw_diff_line_idx}"))?;

        for maybe_lnum_line in maybe_lnum_lines {
            if maybe_lnum_line.starts_with(PATH_LINE_PREFIX) {
                break;
            }
            if !maybe_lnum_line.starts_with("@@ ") {
                continue;
            }

            let lnum = extract_new_lnum_value(maybe_lnum_line)?;

            out.push((path, lnum));
        }
    }

    Ok(out)
}

/// Extracts the line number from a `git diff` hunk header line.
///
/// # Arguments
/// - `lnum_line` The hunk header line (e.g., "@@ -42,7 +42,7 @@", "@@ -42,7 42,7 @@", "@@ -42,7 +42 @@").
///
/// # Returns
/// The line number as a `usize` (e.g., 42).
///
/// # Errors
/// - If the hunk header line lacks sufficient space-separated parts.
/// - If the newline number part is malformed (missing comma).
/// - If the extracted line number value cannot be parsed as a valid [`usize`].
fn extract_new_lnum_value(lnum_line: &str) -> color_eyre::Result<usize> {
    let new_lnum = lnum_line
        .split(' ')
        .nth(2)
        .ok_or_else(|| eyre!("error missing new_lnum from lnum_line after split by space | lnum_line={lnum_line:?}"))?;

    let new_lnum_value = new_lnum
        .split(',')
        .next()
        .and_then(|s| {
            let trimmed = s.trim_start_matches('+');
            if trimmed.is_empty() { None } else { Some(trimmed) }
        })
        .ok_or_else(|| eyre!("error malformed new_lnum in lnum_line | lnum_line={lnum_line:?}"))?;

    new_lnum_value.parse::<usize>().wrap_err_with(|| {
        eyre!("error parsing new_lnum value as usize | lnum_value={new_lnum_value:?}, lnum_line={lnum_line:?}")
    })
}

#[cfg(test)]
mod tests {
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
        vec![("src/main.rs", 42)]
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
        vec![("src/main.rs", 10), ("src/lib.rs", 20)]
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
        vec![("src/main.rs", 10), ("src/main.rs", 50)]
    )]
    #[case::empty_input(vec![], vec![])]
    #[case::no_hunks(
        vec!["diff --git a/src/main.rs b/src/main.rs".to_string()],
        vec![]
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
        vec![("src/main.rs", 42)]
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
        vec![("src/main.rs", 10), ("src/main.rs", 50), ("src/lib.rs", 20), ("src/lib.rs", 60)]
    )]
    fn test_get_hunks_success(#[case] input: Vec<String>, #[case] expected: Vec<(&str, usize)>) {
        assert2::let_assert!(Ok(result) = get_hunks(&input));
        pretty_assertions::assert_eq!(result, expected);
    }

    #[rstest]
    #[case::missing_b_delimiter(
        vec!["diff --git a/src/main.rs".to_string()],
        "error missing path prefix"
    )]
    #[case::invalid_lnum(
        vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "@@ -abc,5 +abc,5 @@".to_string(),
        ],
        "error parsing new_lnum value"
    )]
    fn test_get_hunks_error(#[case] input: Vec<String>, #[case] expected_error_contains: &str) {
        assert2::let_assert!(Err(err) = get_hunks(&input));
        assert!(err.to_string().contains(expected_error_contains));
    }

    #[rstest]
    #[case::standard("@@ -42,7 +42,7 @@", 42)]
    #[case::without_plus("@@ -42,7 42,7 @@", 42)]
    #[case::without_comma("@@ -42,7 +42 @@", 42)]
    #[case::without_plus_or_comma("@@ -42,7 42 @@", 42)]
    fn extract_new_lnum_value_when_valid_lnum_line_returns_correct_usize(#[case] input: &str, #[case] expected: usize) {
        assert2::let_assert!(Ok(result) = extract_new_lnum_value(input));
        pretty_assertions::assert_eq!(result, expected);
    }

    #[rstest]
    #[case::missing_new_lnum_part("@@ -42,7", "error missing new_lnum from lnum_line after split by space")]
    #[case::malformed_lnum("@@ -42,7 +,7 @@", "error malformed new_lnum in lnum_line")]
    #[case::lnum_value_not_numeric("@@ -42,7 +abc,7 @@", "error parsing new_lnum value as usize")]
    fn extract_new_lnum_value_error_cases(#[case] input: &str, #[case] expected_error_contains: &str) {
        assert2::let_assert!(Err(err) = extract_new_lnum_value(input));
        assert!(err.to_string().contains(expected_error_contains));
    }
}
