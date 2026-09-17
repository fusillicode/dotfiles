use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use git2::DiffOptions;
use git2::Patch;
use git2::Repository;
use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_cmd::CmdExt;

const PATH_LINE_PREFIX: &str = "diff --git ";

/// Line additions and removals for one changed file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDiffStats {
    /// Path relative to the repository root.
    pub path: PathBuf,
    /// Number of added lines.
    pub added: usize,
    /// Number of removed lines.
    pub removed: usize,
}

/// Retrieves the current `git diff` raw output with `-U0` as a single `String`.
///
/// Callers should pass the returned string to [`get_hunks`] which splits into lines internally,
/// avoiding per-line `String` allocations.
///
/// # Errors
/// - `git diff` command fails.
pub fn get_raw(path: Option<&Path>) -> rootcause::Result<String> {
    let mut args = vec!["diff".into(), "-U0".into()];

    if let Some(path) = path {
        args.push(path.display().to_string());
    }

    let output = Command::new("git").args(args).exec()?;

    ytil_cmd::extract_success_output(&output)
}

/// Retrieves line additions and removals for tracked files changed from `HEAD`.
///
/// The repository's `HEAD` tree is compared with its index and working tree. Staged and unstaged
/// changes are included. Untracked files and binary files do not produce line statistics.
///
/// # Errors
/// - The repository, its `HEAD` tree, or its working tree diff cannot be read.
pub fn get_line_stats(repo_root: &Path) -> rootcause::Result<Vec<FileDiffStats>> {
    let repo = Repository::open(repo_root)
        .context("error opening repository")
        .attach_with(|| format!("repo_root={}", repo_root.display()))?;
    let head_tree = repo
        .head()
        .context("error reading repository HEAD")
        .attach_with(|| format!("repo_root={}", repo_root.display()))?
        .peel_to_tree()
        .context("error reading repository HEAD tree")
        .attach_with(|| format!("repo_root={}", repo_root.display()))?;
    let diff = repo
        .diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut DiffOptions::new()))
        .context("error creating repository worktree diff")
        .attach_with(|| format!("repo_root={}", repo_root.display()))?;

    let mut out = Vec::with_capacity(diff.deltas().len());
    for (idx, delta) in diff.deltas().enumerate() {
        let Some(file_patch) = Patch::from_diff(&diff, idx)
            .context("error creating file diff patch")
            .attach_with(|| format!("repo_root={} diff_idx={idx}", repo_root.display()))?
        else {
            // Binary and unchanged files have no line statistics.
            continue;
        };

        let Some(changed_path) = delta.new_file().path().or_else(|| delta.old_file().path()) else {
            continue;
        };
        let (_, added, removed) = file_patch
            .line_stats()
            .context("error reading file diff line statistics")
            .attach_with(|| format!("repo_root={} path={}", repo_root.display(), changed_path.display()))?;

        out.push(FileDiffStats {
            path: changed_path.to_path_buf(),
            added,
            removed,
        });
    }

    Ok(out)
}

/// Extracts file paths and starting line numbers of hunks from raw `git diff` output.
///
/// Accepts a `&str` (the full diff output) and splits into lines internally, avoiding
/// per-line `String` allocations.
///
/// # Errors
/// - Parsing diff output fails.
pub fn get_hunks(raw_diff_output: &str) -> rootcause::Result<Vec<(&str, usize)>> {
    let lines: Vec<&str> = raw_diff_output.lines().collect();

    // Pre-allocate with estimated capacity: roughly 1 hunk per 4 diff lines
    let mut out = Vec::with_capacity(lines.len().saturating_div(4).max(1));

    for (raw_diff_line_idx, raw_diff_line) in lines.iter().enumerate() {
        let Some(path_line) = raw_diff_line.strip_prefix(PATH_LINE_PREFIX) else {
            continue;
        };

        let path_idx = path_line
            .find(" b/")
            .ok_or_else(|| report!("error missing path prefix in path_line"))
            .attach_with(|| {
                format!("path_line={path_line:?} raw_diff_line_idx={raw_diff_line_idx} raw_diff_line={raw_diff_line:?}")
            })?
            .saturating_add(3);

        let path = path_line.get(path_idx..)
            .ok_or_else(|| report!("error extracting path from path_line"))
            .attach_with(|| format!("path_idx={path_idx} path_line={path_line:?} raw_diff_line_idx={raw_diff_line_idx} raw_diff_line={raw_diff_line:?}"))?;

        let lnum_lines_start_idx = raw_diff_line_idx.saturating_add(1);
        let maybe_lnum_lines = lines
            .get(lnum_lines_start_idx..)
            .ok_or_else(|| report!("error extracting lnum_lines from raw_diff_output"))
            .attach_with(|| {
                format!("lnum_lines_start_idx={lnum_lines_start_idx} raw_diff_line_idx={raw_diff_line_idx}")
            })?;

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
/// # Errors
/// - If the hunk header line lacks sufficient space-separated parts.
/// - If the newline number part is malformed (missing comma).
/// - If the extracted line number value cannot be parsed as a valid [`usize`].
fn extract_new_lnum_value(lnum_line: &str) -> rootcause::Result<usize> {
    let new_lnum = lnum_line
        .split(' ')
        .nth(2)
        .ok_or_else(|| report!("error missing new_lnum from lnum_line after split by space"))
        .attach_with(|| format!("lnum_line={lnum_line:?}"))?;

    let new_lnum_value = new_lnum
        .split(',')
        .next()
        .and_then(|s| {
            let trimmed = s.trim_start_matches('+');
            if trimmed.is_empty() { None } else { Some(trimmed) }
        })
        .ok_or_else(|| report!("error malformed new_lnum in lnum_line"))
        .attach_with(|| format!("lnum_line={lnum_line:?}"))?;

    Ok(new_lnum_value
        .parse::<usize>()
        .context("error parsing new_lnum value as usize")
        .attach_with(|| format!("lnum_value={new_lnum_value:?} lnum_line={lnum_line:?}"))?)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case::single_file_single_hunk(
        "diff --git a/src/main.rs b/src/main.rs\nindex 1234567..abcdef0 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -42,7 +42,7 @@",
        vec![("src/main.rs", 42)]
    )]
    #[case::multiple_files(
        "diff --git a/src/main.rs b/src/main.rs\nindex 1234567..abcdef0 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -10,5 +10,5 @@\ndiff --git a/src/lib.rs b/src/lib.rs\nindex fedcba9..7654321 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -20,3 +20,3 @@",
        vec![("src/main.rs", 10), ("src/lib.rs", 20)]
    )]
    #[case::multiple_hunks_same_file(
        "diff --git a/src/main.rs b/src/main.rs\nindex 1234567..abcdef0 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -10,5 +10,5 @@\n@@ -50,2 +50,2 @@",
        vec![("src/main.rs", 10), ("src/main.rs", 50)]
    )]
    #[case::empty_input("", vec![])]
    #[case::no_hunks(
        "diff --git a/src/main.rs b/src/main.rs",
        vec![]
    )]
    #[case::non_diff_lines_ignored(
        "index 123..456 789\ndiff --git a/src/main.rs b/src/main.rs\nindex 1234567..abcdef0 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -42,7 +42,7 @@",
        vec![("src/main.rs", 42)]
    )]
    #[case::multiple_files_with_multiple_hunks(
        "diff --git a/src/main.rs b/src/main.rs\nindex 1234567..abcdef0 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -10,5 +10,5 @@\n@@ -50,2 +50,2 @@\ndiff --git a/src/lib.rs b/src/lib.rs\nindex fedcba9..7654321 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -20,3 +20,3 @@\n@@ -60,1 +60,1 @@",
        vec![("src/main.rs", 10), ("src/main.rs", 50), ("src/lib.rs", 20), ("src/lib.rs", 60)]
    )]
    fn test_get_hunks_success(#[case] input: &str, #[case] expected: Vec<(&str, usize)>) {
        assert_that!(get_hunks(input), ok(eq(expected)));
    }

    #[rstest]
    #[case::missing_b_delimiter("diff --git a/src/main.rs", "error missing path prefix")]
    #[case::invalid_lnum(
        "diff --git a/src/main.rs b/src/main.rs\n@@ -abc,5 +abc,5 @@",
        "error parsing new_lnum value"
    )]
    fn test_get_hunks_error(#[case] input: &str, #[case] expected_error_contains: &str) {
        assert_that!(
            (get_hunks(input)).map(|_| ()),
            err(displays_as(contains_substring(expected_error_contains)))
        );
    }

    #[rstest]
    #[case::standard("@@ -42,7 +42,7 @@", 42)]
    #[case::without_plus("@@ -42,7 42,7 @@", 42)]
    #[case::without_comma("@@ -42,7 +42 @@", 42)]
    #[case::without_plus_or_comma("@@ -42,7 42 @@", 42)]
    fn test_extract_new_lnum_value_when_valid_lnum_line_returns_correct_usize(
        #[case] input: &str,
        #[case] expected: usize,
    ) {
        assert_that!(extract_new_lnum_value(input), ok(eq(expected)));
    }

    #[rstest]
    #[case::missing_new_lnum_part("@@ -42,7", "error missing new_lnum from lnum_line after split by space")]
    #[case::malformed_lnum("@@ -42,7 +,7 @@", "error malformed new_lnum in lnum_line")]
    #[case::lnum_value_not_numeric("@@ -42,7 +abc,7 @@", "error parsing new_lnum value as usize")]
    fn test_extract_new_lnum_value_when_input_invalid_returns_expected_error(
        #[case] input: &str,
        #[case] expected_error_contains: &str,
    ) {
        assert_that!(
            (extract_new_lnum_value(input)).map(|_| ()),
            err(displays_as(contains_substring(expected_error_contains)))
        );
    }

    #[test]
    fn test_get_line_stats_when_staged_and_unstaged_changes_exist_includes_both() {
        let (temp_dir, repo) = crate::tests::init_test_repo(None);
        let relative_path = Path::new("src/main.rs");
        let absolute_path = temp_dir.path().join(relative_path);

        fs::create_dir_all(absolute_path.parent().unwrap()).unwrap();
        fs::write(&absolute_path, "one\n").unwrap();
        commit_file(&repo, relative_path);

        fs::write(&absolute_path, "one\ntwo\n").unwrap();
        stage_file(&repo, relative_path);
        fs::write(&absolute_path, "one\ntwo\nthree\n").unwrap();

        assert_that!(
            get_line_stats(temp_dir.path()),
            ok(eq(vec![FileDiffStats {
                path: relative_path.into(),
                added: 2,
                removed: 0,
            }]))
        );
    }

    fn commit_file(repo: &Repository, relative_path: &Path) {
        stage_file(repo, relative_path);
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("test", "test@example.com").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "add file", &tree, &[&parent])
            .unwrap();
    }

    fn stage_file(repo: &Repository, relative_path: &Path) {
        let mut index = repo.index().unwrap();
        index.add_path(relative_path).unwrap();
        index.write().unwrap();
    }
}
