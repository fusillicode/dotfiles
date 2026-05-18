use std::path::PathBuf;

use crate::plugin::tbar::Event;
use crate::plugin::tbar::TbarState;

pub fn derive(state: &TbarState, requested_cwd: &PathBuf, exit_code: Option<i32>, stdout: &[u8]) -> Vec<Event> {
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
    let Ok(records) = agg::parse_git_stat_records(&output).inspect_err(|error| eprintln!("agg: {error}")) else {
        return vec![];
    };
    for new_stat in records {
        if new_stat.path != *requested_cwd {
            continue;
        }
        if current_tab.git_stat == new_stat {
            return vec![];
        }
        return vec![Event::GitStatChanged { new_stat }];
    }

    vec![]
}
