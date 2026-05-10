use std::path::PathBuf;

use agg::GitStat;

use crate::plugin::picker::state::PickerEvent;

pub fn derive(requested_cwd: &PathBuf, exit_code: Option<i32>, stdout: &[u8]) -> Vec<PickerEvent> {
    if exit_code != Some(0) {
        return vec![];
    }

    let output = String::from_utf8_lossy(stdout);
    for line in output.lines() {
        let Ok(stat) = line
            .parse::<GitStat>()
            .inspect_err(|error| eprintln!("agg picker: {error}"))
        else {
            continue;
        };
        if stat.path != *requested_cwd {
            continue;
        }
        return vec![PickerEvent::GitStatUpdated { stat }];
    }

    vec![]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use agg::GitStat;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_derive_returns_branch_and_stat_for_matching_requested_cwd() {
        let cwd = PathBuf::from("/tmp/repo");
        let stat = GitStat {
            path: cwd.clone(),
            branch: Some("main".to_string()),
            insertions: 2,
            deletions: 1,
            new_files: 3,
            is_worktree: false,
        };
        let stdout = stat.to_string();

        let events = derive(&cwd, Some(0), stdout.as_bytes());

        assert_eq!(events, vec![PickerEvent::GitStatUpdated { stat }]);
    }

    #[test]
    fn test_derive_ignores_other_cwds() {
        let cwd = PathBuf::from("/tmp/repo");
        let stdout = GitStat {
            path: PathBuf::from("/tmp/other"),
            branch: Some("main".to_string()),
            insertions: 2,
            deletions: 1,
            new_files: 3,
            is_worktree: false,
        }
        .to_string();

        let events = derive(&cwd, Some(0), stdout.as_bytes());

        assert_eq!(events, vec![]);
    }
}
