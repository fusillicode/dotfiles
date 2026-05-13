use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::str::FromStr;

use agg::GitStat;
use agg::LastCommit;

const COMMIT_SUMMARY_MAX_WIDTH: usize = 80;
const SHORT_SHA_LEN: usize = 7;

/// Git stat collection mode.
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum UseCase {
    Tbar,
    Ppick,
}

/// Error returned when a git stat collection mode is unknown.
pub struct UseCaseParseError {
    value: String,
}

impl Display for UseCaseParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "unknown git-stat use case {:?}, expected tbar or ppick", self.value)
    }
}

impl FromStr for UseCase {
    type Err = UseCaseParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "tbar" => Ok(Self::Tbar),
            "ppick" => Ok(Self::Ppick),
            _ => Err(UseCaseParseError {
                value: value.to_string(),
            }),
        }
    }
}

pub fn run(cwd: &str, use_case: UseCase) -> GitStat {
    let path = cwd.into();
    let Ok(repo) = git2::Repository::discover(cwd) else {
        return GitStat {
            path,
            branch: None,
            ..Default::default()
        };
    };

    let (branch, last_commit) = match use_case {
        UseCase::Tbar => (None, None),
        UseCase::Ppick => {
            let branch = repo
                .head()
                .ok()
                .filter(git2::Reference::is_branch)
                .and_then(|head| head.shorthand().map(str::to_string));
            (branch, last_commit_metadata(&repo))
        }
    };

    let (insertions, deletions) = repo
        .diff_index_to_workdir(None, None)
        .and_then(|d| d.stats())
        .map_or((0, 0), |s| (s.insertions(), s.deletions()));

    let new_files = repo
        .statuses(Some(
            git2::StatusOptions::new()
                .include_untracked(true)
                .recurse_untracked_dirs(true)
                .exclude_submodules(true)
                .include_ignored(false),
        ))
        .map_or(0, |st| {
            st.iter().filter(|s| s.status().contains(git2::Status::WT_NEW)).count()
        });

    GitStat {
        path,
        branch,
        last_commit,
        insertions,
        deletions,
        new_files,
        ..Default::default()
    }
}

fn last_commit_metadata(repo: &git2::Repository) -> Option<LastCommit> {
    let Ok(commit) = repo.head().and_then(|head| head.peel_to_commit()) else {
        return None;
    };
    let short_sha = commit.id().to_string().chars().take(SHORT_SHA_LEN).collect();
    let age = commit_age_label(commit.time().seconds(), chrono::Utc::now().timestamp());
    let summary = commit
        .summary()
        .map(|summary| ytil_tui::display_fixed_width(summary, COMMIT_SUMMARY_MAX_WIDTH))
        .unwrap_or_default();
    Some(LastCommit {
        short_sha,
        age,
        summary,
    })
}

fn commit_age_label(committed_at: i64, now: i64) -> String {
    let elapsed = now.saturating_sub(committed_at).max(0);
    match elapsed {
        0..=59 => format!("{elapsed}s"),
        60..=3_599 => format!("{}m", elapsed / 60),
        3_600..=86_399 => format!("{}h", elapsed / 3_600),
        86_400..=604_799 => format!("{}d", elapsed / 86_400),
        604_800..=31_535_999 => format!("{}w", elapsed / 604_800),
        _ => format!("{}y", elapsed / 31_536_000),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("tbar", UseCase::Tbar)]
    #[case("ppick", UseCase::Ppick)]
    fn test_use_case_from_str_when_known_returns_use_case(#[case] input: &str, #[case] expected: UseCase) {
        assert2::assert!(let Ok(actual) = input.parse::<UseCase>());

        pretty_assertions::assert_eq!(actual, expected);
    }

    #[test]
    fn test_use_case_from_str_when_unknown_returns_descriptive_error() {
        assert2::assert!(let Err(err) = "full".parse::<UseCase>());

        pretty_assertions::assert_eq!(
            err.to_string(),
            "unknown git-stat use case \"full\", expected tbar or ppick"
        );
    }

    #[rstest]
    #[case(1_000, 1_125, "2m")]
    #[case(1_000, 87_400, "1d")]
    #[case(1_000, 605_800, "1w")]
    fn test_commit_age_label_formats_relative_age_units(
        #[case] committed_at: i64,
        #[case] now: i64,
        #[case] expected: &str,
    ) {
        pretty_assertions::assert_eq!(commit_age_label(committed_at, now), expected);
    }
}
