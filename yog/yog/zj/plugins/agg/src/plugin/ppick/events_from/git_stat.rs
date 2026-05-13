use std::path::PathBuf;

use crate::plugin::ppick::state::PpickEvent;

pub fn derive(requested_cwd: &PathBuf, exit_code: Option<i32>, stdout: &[u8]) -> Vec<PpickEvent> {
    if exit_code != Some(0) {
        return vec![];
    }

    let output = String::from_utf8_lossy(stdout);
    let Ok(stats) = agg::parse_git_stat_records(&output).inspect_err(|error| eprintln!("agg ppick: {error}")) else {
        return vec![];
    };
    for stat in stats {
        if stat.path != *requested_cwd {
            continue;
        }
        return vec![PpickEvent::GitStatUpdated { stat }];
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
            ..Default::default()
        };
        let stdout = stat.to_string();

        let events = derive(&cwd, Some(0), stdout.as_bytes());

        assert_eq!(events, vec![PpickEvent::GitStatUpdated { stat }]);
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
            ..Default::default()
        }
        .to_string();

        let events = derive(&cwd, Some(0), stdout.as_bytes());

        assert_eq!(events, vec![]);
    }
}
