use std::path::PathBuf;

use agg::GitStat;

use crate::plugin::events::StateEvent;
use crate::plugin::state::State;

pub fn derive(state: &State, requested_cwd: &PathBuf, exit_code: Option<i32>, stdout: &[u8]) -> Vec<StateEvent> {
    if exit_code != Some(0) {
        return vec![];
    }

    let Some(current_tab) = state.current_tab.as_ref() else {
        return vec![];
    };
    if current_tab.cwd.as_ref() != Some(requested_cwd) {
        return vec![];
    }

    let output = String::from_utf8_lossy(stdout);
    for line in output.lines() {
        let Ok((path, new_stat)) = GitStat::parse_line(line).inspect_err(|error| eprintln!("agg: {error}")) else {
            continue;
        };
        if path != *requested_cwd {
            continue;
        }
        if current_tab.git_stat == new_stat {
            return vec![];
        }
        return vec![StateEvent::GitStatChanged { new_stat }];
    }

    vec![]
}
