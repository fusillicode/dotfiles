use std::collections::HashMap;
use std::process::Command;

use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use ytil_cmd::CmdExt;

pub fn get() -> color_eyre::Result<Vec<String>> {
    let output = Command::new("git").arg("diff").exec()?;

    Ok(ytil_cmd::extract_success_output(&output)?
        .lines()
        .map(|s| s.to_string())
        .collect())
}

pub fn get_paths_with_lnums(diff_output: &[String]) -> color_eyre::Result<HashMap<&str, Vec<usize>>> {
    let mut out: HashMap<&str, Vec<usize>> = HashMap::new();

    for (diff_line_idx, diff_line) in diff_output.iter().enumerate() {
        let Some(path_line) = diff_line.strip_prefix("diff --git") else {
            continue;
        };

        let path_idx = path_line
            .find(" b/")
            .ok_or_else(|| {
                eyre!(
                    "error missing path delimiter in path_line | path_line={path_line:?} diff_line_idx={diff_line_idx} diff_line={diff_line:?}"
                )
            })?
            .saturating_add(3);

        let path = path_line.get(path_idx..).ok_or_else(|| {
            eyre!("error extracting path from path_line | path_idx={path_idx} path_line={path_line:?} diff_line_idx={diff_line_idx} diff_line={diff_line:?}")
        })?;

        let lnum_lines_start_idx = diff_line_idx + 1;
        for maybe_lnum_line in diff_output
            .get(lnum_lines_start_idx..)
            .ok_or_else(|| eyre!("error extracting lnum_lines from diff_output | lnum_lines_start_idx={lnum_lines_start_idx} diff_line_idx={diff_line_idx}"))?
        {
            if maybe_lnum_line.starts_with("diff --git") {
                break;
            }
            let Some(lnum_line) = maybe_lnum_line.strip_prefix("@@ -") else {
                continue;
            };

            let lnum_idx = lnum_line
                .find(',')
                .ok_or_else(|| eyre!("error missing lnum delimiter in lnum_line | lnum_line={lnum_line:?} diff_line_idx={lnum_lines_start_idx} diff_line={diff_line:?}"))?;
            let lnum = {
                let lnum_value = lnum_line.get(..lnum_idx).ok_or_else(|| {
                    eyre!("error extracting lnum from lnum_line | lnum_idx={lnum_idx} lnum_line={lnum_line:?} diff_line_idx={lnum_lines_start_idx} diff_line={diff_line:?}")
                })?;
                lnum_value
                    .parse()
                    .wrap_err_with(|| eyre!("error parsing lnum value as usize | lnum_value={lnum_value:?}"))?
            };

            out.entry(path)
                .and_modify(|lines_numbers| lines_numbers.push(lnum))
                .or_insert_with(|| vec![lnum]);
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

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
        HashMap::from([("src/main.rs", vec![42])])
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
        HashMap::from([("src/main.rs", vec![10]), ("src/lib.rs", vec![20])])
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
        HashMap::from([("src/main.rs", vec![10, 50])])
    )]
    #[case::empty_input(vec![], HashMap::new())]
    #[case::no_hunks(
        vec!["diff --git a/src/main.rs b/src/main.rs".to_string()],
        HashMap::new()
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
        HashMap::from([("src/main.rs", vec![42])])
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
        HashMap::from([("src/main.rs", vec![10, 50]), ("src/lib.rs", vec![20, 60])])
    )]
    fn test_get_paths_with_lnums_success(#[case] input: Vec<String>, #[case] expected: HashMap<&str, Vec<usize>>) {
        assert2::let_assert!(Ok(result) = get_paths_with_lnums(&input));
        pretty_assertions::assert_eq!(result, expected);
    }

    #[rstest]
    #[case::missing_b_delimiter(
        vec!["diff --git a/src/main.rs".to_string()],
        "error missing path delimiter"
    )]
    #[case::missing_comma_in_hunk(
        vec![
            "diff --git a/src/main.rs b/src/main.rs".to_string(),
            "@@ -42 +42 @@".to_string(),
        ],
        "error missing lnum delimiter"
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
}
