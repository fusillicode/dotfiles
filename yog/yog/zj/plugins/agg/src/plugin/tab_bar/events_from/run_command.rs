use std::path::PathBuf;

use agg::GitStat;

use crate::plugin::tab_bar::Event;
use crate::plugin::tab_bar::TabBarState;

pub fn derive(state: &TabBarState, requested_cwd: &PathBuf, exit_code: Option<i32>, stdout: &[u8]) -> Vec<Event> {
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
        let Ok(new_stat) = line.parse::<GitStat>().inspect_err(|error| eprintln!("agg: {error}")) else {
            continue;
        };
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
