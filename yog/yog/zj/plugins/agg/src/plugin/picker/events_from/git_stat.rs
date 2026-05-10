use std::path::PathBuf;

use agg::GitStat;

use crate::plugin::picker::state::PickerEvent;

pub fn derive(requested_cwd: &PathBuf, exit_code: Option<i32>, stdout: &[u8]) -> Vec<PickerEvent> {
    if exit_code != Some(0) {
        return vec![];
    }

    let output = String::from_utf8_lossy(stdout);
    for line in output.lines() {
        let Ok((cwd, stat)) = GitStat::parse_line(line).inspect_err(|error| eprintln!("agg picker: {error}")) else {
            continue;
        };
        if cwd != *requested_cwd {
            continue;
        }
        return vec![PickerEvent::GitStatUpdated { cwd, stat }];
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
    fn test_derive_git_stat_returns_update_for_matching_requested_cwd() {
        let cwd = PathBuf::from("/tmp/repo");
        let stdout = b"/tmp/repo 2 1 3 0\n/tmp/other 4 5 6 0\n";

        let events = derive(&cwd, Some(0), stdout);

        assert_eq!(
            events,
            vec![PickerEvent::GitStatUpdated {
                cwd,
                stat: GitStat {
                    insertions: 2,
                    deletions: 1,
                    new_files: 3,
                    is_worktree: false,
                },
            }]
        );
    }
}
