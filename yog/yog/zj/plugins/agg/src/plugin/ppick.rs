use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use zellij_tile::prelude::EventType;
use zellij_tile::prelude::KeyWithModifier;
use zellij_tile::prelude::MessageToPlugin;
use zellij_tile::prelude::PaneId;
use zellij_tile::prelude::PaneManifest;
use zellij_tile::prelude::PipeMessage;
use zellij_tile::prelude::TabInfo;

use crate::plugin::ppick::state::PpickAction;
use crate::plugin::ppick::state::PpickState;
use crate::plugin::tbar::AGG_SYNC_PIPE;
use crate::plugin::tbar::StateSnapshotPayload;

mod decode;
mod entry;
pub mod state;
pub mod ui;

// Unlike tbar, ppick does not keep a semantic event pipeline. Tbar fans one host
// input into tab, sync, git-stat, and nudge transitions; ppick inputs map to one
// local state mutation plus local side effects, so raw decoding stays in `decode`
// and mutation stays on `PpickState`.
const CONTEXT_KIND: &str = "kind";
const CONTEXT_AGS_SESSIONS: &str = "ags_sessions";
const CONTEXT_GIT_STAT: &str = "git_stat";
const CONTEXT_CWD: &str = "cwd";

pub fn load(state: &mut PpickState, home_dir: PathBuf, config: &BTreeMap<String, String>) {
    state.home_dir = home_dir;
    state.set_floating_coordinates(
        config.get("y").cloned(),
        config.get("width").cloned(),
        config.get("height").cloned(),
    );
}

pub fn update_permission_granted(state: &mut PpickState) -> bool {
    zellij_tile::prelude::subscribe(&[
        EventType::TabUpdate,
        EventType::PaneUpdate,
        EventType::PaneClosed,
        EventType::CwdChanged,
        EventType::CommandChanged,
        EventType::Key,
        EventType::RunCommandResult,
    ]);
    let mut sync_args = BTreeMap::new();
    sync_args.insert("type".to_string(), "sync_request".to_string());
    zellij_tile::prelude::pipe_message_to_plugin(MessageToPlugin::new(AGG_SYNC_PIPE.to_string()).with_args(sync_args));
    zellij_tile::prelude::set_selectable(true);
    apply_floating_coordinates(state);
    crate::plugin::ppick::run_ags_sessions();
    true
}

pub fn render(state: &mut PpickState, rows: usize, cols: usize, buf: &mut String) {
    let capacity = rows.saturating_sub(1) / crate::plugin::ppick::ui::ENTRY_ROWS;
    let frame = state.visible_frame(capacity);
    crate::plugin::ppick::ui::render_frame(&frame, &state.query, rows, cols, buf);
}

pub fn update_tabs(state: &mut PpickState, tabs: Vec<TabInfo>) -> bool {
    let changed = state.update_tabs(tabs);
    let coordinates_changed = apply_floating_coordinates(state);
    changed || coordinates_changed
}

pub fn update_panes(state: &mut PpickState, manifest: &PaneManifest) -> bool {
    let changed = state.update_panes(
        manifest,
        |pane_id| zellij_tile::prelude::get_pane_cwd(PaneId::Terminal(pane_id)).ok(),
        |pane_id| zellij_tile::prelude::get_pane_running_command(PaneId::Terminal(pane_id)).ok(),
    );
    if changed {
        run_git_stats(state);
    }
    if state.should_close_empty_picker() {
        zellij_tile::prelude::close_self();
        return false;
    }
    changed
}

pub fn update_pane_closed(state: &mut PpickState, pane_id: u32) -> bool {
    state.remove_pane(pane_id)
}

pub fn update_cwd(state: &mut PpickState, pane_id: u32, cwd: &Path) -> bool {
    let changed = state.update_cwd(pane_id, cwd);
    if changed {
        run_git_stats(state);
    }
    changed
}

pub fn update_command(state: &mut PpickState, pane_id: PaneId, command: &[String], is_foreground: bool) -> bool {
    if !is_foreground {
        return false;
    }
    let PaneId::Terminal(pane_id) = pane_id else {
        return false;
    };
    state.update_command(pane_id, command)
}

pub fn update_key(state: &mut PpickState, key: &KeyWithModifier) -> bool {
    let action = state.handle_key(key);
    match action {
        PpickAction::Close => {
            zellij_tile::prelude::close_self();
            false
        }
        PpickAction::Focus(pane_id) => {
            zellij_tile::prelude::focus_pane_with_id(PaneId::Terminal(pane_id), true, false);
            zellij_tile::prelude::close_self();
            false
        }
        PpickAction::Redraw => true,
        PpickAction::None => false,
    }
}

pub fn pipe(state: &mut PpickState, pipe_message: &PipeMessage) -> bool {
    let is_state_snapshot = pipe_message.name == AGG_SYNC_PIPE
        && pipe_message.args.get("type").map(String::as_str) == Some("state_snapshot");
    if is_state_snapshot {
        let Ok(snapshot) =
            StateSnapshotPayload::try_from(pipe_message).inspect_err(|error| eprintln!("agg ppick: {error}"))
        else {
            return false;
        };
        return state.apply_state_snapshot(&snapshot);
    }
    crate::plugin::ppick::decode::agent_event_from_pipe(pipe_message).is_some_and(|event| state.update_agent(&event))
}

pub fn update_run_command_result(
    state: &mut PpickState,
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
    context: &BTreeMap<String, String>,
) -> bool {
    match context.get(CONTEXT_KIND).map(String::as_str) {
        Some(CONTEXT_AGS_SESSIONS) => {
            if exit_code != Some(0) {
                eprintln!("agg ppick: ags list --json failed: {}", String::from_utf8_lossy(stderr));
                return false;
            }
            match crate::plugin::ppick::decode::sessions_from_stdout(stdout) {
                Ok(entries) => state.update_sessions(entries),
                Err(err) => {
                    eprintln!("agg ppick: failed to parse ags sessions: {err}");
                    false
                }
            }
        }
        Some(CONTEXT_GIT_STAT) => {
            let Some(cwd) = context.get(CONTEXT_CWD).map(PathBuf::from) else {
                return false;
            };
            state.finish_git_stat_request(&cwd);
            crate::plugin::ppick::decode::git_stat_from_run_command(&cwd, exit_code, stdout)
                .is_some_and(|stat| state.update_git_stat(&stat))
        }
        Some(_) | None => false,
    }
}

fn apply_floating_coordinates(state: &mut PpickState) -> bool {
    let Some(coordinates) = state.take_floating_coordinates() else {
        return false;
    };
    let plugin_id = zellij_tile::prelude::get_plugin_ids().plugin_id;
    zellij_tile::prelude::change_floating_panes_coordinates(vec![(PaneId::Plugin(plugin_id), coordinates)]);
    true
}

fn run_ags_sessions() {
    let mut context = BTreeMap::new();
    context.insert(CONTEXT_KIND.to_string(), CONTEXT_AGS_SESSIONS.to_string());
    zellij_tile::prelude::run_command(&["ags", "list", "--json"], context);
}

fn run_git_stats(state: &mut PpickState) {
    for cwd in state.take_git_stat_cwds_to_request() {
        crate::plugin::ppick::run_git_stat(cwd);
    }
}

fn run_git_stat(cwd: PathBuf) {
    let cwd_str = cwd.display().to_string();
    let mut context = BTreeMap::new();
    context.insert(CONTEXT_KIND.to_string(), CONTEXT_GIT_STAT.to_string());
    context.insert(CONTEXT_CWD.to_string(), cwd_str.clone());
    let args: Vec<&str> = vec!["agg", "git-stat", "ppick", &cwd_str];
    zellij_tile::prelude::run_command_with_env_variables_and_cwd(&args, BTreeMap::new(), cwd, context);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use agg::Cmd;
    use agg::GitStat;
    use agg::TabIndicator;
    use agg::TabStateEntry;
    use pretty_assertions::assert_eq;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;
    use zellij_tile::prelude::PipeMessage;
    use zellij_tile::prelude::PipeSource;
    use zellij_tile::prelude::TabInfo;

    use super::*;

    fn terminal_pane_with_command(id: u32, command: &str) -> PaneInfo {
        PaneInfo {
            id,
            terminal_command: Some(command.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_pipe_state_snapshot_selects_focused_pane_for_active_tab() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 20,
                position: 0,
                active: true,
                ..Default::default()
            },
            TabInfo {
                tab_id: 10,
                position: 1,
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: [
                (
                    0,
                    vec![
                        terminal_pane_with_command(42, "cargo"),
                        terminal_pane_with_command(43, "nvim"),
                    ],
                ),
                (1, vec![terminal_pane_with_command(44, "zsh")]),
            ]
            .into_iter()
            .collect(),
        };
        let _ = state.update_panes(&manifest, |_| None, |_| None);
        let msg = PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGG_SYNC_PIPE.to_string(),
            payload: Some(
                TabStateEntry {
                    tab_id: 20,
                    cwd: None,
                    cmd: Cmd::None,
                    indicator: TabIndicator::NoAgent,
                    git_stat: GitStat::default(),
                }
                .to_string(),
            ),
            args: BTreeMap::from([
                (String::from("type"), String::from("state_snapshot")),
                (String::from("tab_id"), String::from("20")),
                (String::from("seq"), String::from("1")),
                (String::from("focused_pane_id"), String::from("43")),
            ]),
            is_private: false,
        };

        assert2::assert!(pipe(&mut state, &msg));

        assert_eq!(
            state.visible_frame(usize::MAX).get(1).map(|row| row.selected),
            Some(true)
        );
    }
}
