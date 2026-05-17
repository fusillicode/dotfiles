use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use agg::GitStat;
use serde::Deserialize;
use zellij_tile::prelude::BareKey;
use zellij_tile::prelude::KeyModifier;
use zellij_tile::prelude::KeyWithModifier;
use zellij_tile::prelude::TabInfo;

use crate::plugin::ppick::entry::PaneEntry;
use crate::plugin::tbar::PaneAgentSnapshot;

mod agents;
mod floating;
mod git;
mod panes;
mod selection;
mod sessions;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PpickMode {
    #[default]
    AllPanes,
    AgentsOnly,
}

impl PpickMode {
    const fn includes_entry(self, entry: &PaneEntry) -> bool {
        match self {
            Self::AllPanes => true,
            Self::AgentsOnly => entry.is_agent_pane(),
        }
    }
}

#[derive(Default)]
pub struct PpickState {
    mode: PpickMode,
    pub home_dir: PathBuf,
    pub query: String,
    selected: usize,
    selected_pane_id: Option<u32>,
    filtered_entry_indices: Vec<usize>,
    filter_ready: bool,
    pane_entries: Vec<PaneEntry>,
    sessions_by_key: HashMap<(String, String), SessionEntry>,
    cwds_by_pane: HashMap<u32, PathBuf>,
    commands_by_pane: HashMap<u32, Vec<String>>,
    agent_snapshots_by_pane: HashMap<u32, PaneAgentSnapshot>,
    git_stats_by_cwd: HashMap<PathBuf, GitStat>,
    git_stat_cwds_to_refresh: HashSet<PathBuf>,
    git_stat_cwds_in_flight: HashSet<PathBuf>,
    all_tabs: Vec<TabInfo>,
    floating_y: Option<String>,
    floating_width: Option<String>,
    floating_height: Option<String>,
    floating_display_rows: Option<usize>,
    floating_display_columns: Option<usize>,
    floating_size_applied: bool,
    initial_focus_by_tab: HashMap<usize, InitialFocus>,
    selection_touched: bool,
}

#[derive(Clone, Copy)]
struct InitialFocus {
    pane_id: u32,
    seq: u64,
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum PpickAction {
    None,
    Redraw,
    Close,
    Focus(u32),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SessionEntry {
    pub agent: String,
    pub workspace: PathBuf,
    pub session_id: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub display: String,
    pub search: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneObservation {
    pub tab_position: usize,
    pub pane_id: u32,
    pub terminal_command_args: Option<Vec<String>>,
    pub title_label: Option<String>,
    pub is_focused: bool,
    cwd: Option<PathBuf>,
    command: Option<Vec<String>>,
}

impl PpickState {
    pub fn new(mode: PpickMode) -> Self {
        Self {
            mode,
            ..Default::default()
        }
    }

    pub fn handle_key(&mut self, key: &KeyWithModifier) -> PpickAction {
        match key.bare_key {
            BareKey::Esc if key.has_no_modifiers() => PpickAction::Close,
            BareKey::Enter if key.has_no_modifiers() => {
                self.ensure_filter();
                self.selected_entry()
                    .map_or(PpickAction::None, |entry| PpickAction::Focus(entry.pane_id))
            }
            BareKey::Backspace if key.has_no_modifiers() => {
                if self.query.pop().is_none() {
                    return PpickAction::None;
                }
                self.selection_touched = true;
                self.mark_filter_dirty();
                self.clamp_selection();
                PpickAction::Redraw
            }
            BareKey::Down if key.has_no_modifiers() => self.select_next(),
            BareKey::Char('n') if key.has_only_modifiers(&[KeyModifier::Ctrl]) => self.select_next(),
            BareKey::Up if key.has_no_modifiers() => self.select_previous(),
            BareKey::Char('p') if key.has_only_modifiers(&[KeyModifier::Ctrl]) => self.select_previous(),
            BareKey::Char(c)
                if !c.is_control() && (key.has_no_modifiers() || key.has_only_modifiers(&[KeyModifier::Shift])) =>
            {
                self.query.push(c);
                self.selection_touched = true;
                self.mark_filter_dirty();
                self.clamp_selection();
                PpickAction::Redraw
            }
            BareKey::PageDown
            | BareKey::PageUp
            | BareKey::Left
            | BareKey::Right
            | BareKey::Home
            | BareKey::End
            | BareKey::Delete
            | BareKey::Insert
            | BareKey::F(_)
            | BareKey::Tab
            | BareKey::CapsLock
            | BareKey::ScrollLock
            | BareKey::NumLock
            | BareKey::PrintScreen
            | BareKey::Pause
            | BareKey::Menu
            | BareKey::Esc
            | BareKey::Enter
            | BareKey::Backspace
            | BareKey::Down
            | BareKey::Up
            | BareKey::Char(_) => PpickAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use ytil_agents::agent::Agent;
    use ytil_agents::agent::AgentEventKind;
    use ytil_agents::agent::AgentEventPayload;
    use zellij_tile::prelude::BareKey;
    use zellij_tile::prelude::FloatingPaneCoordinates;
    use zellij_tile::prelude::KeyModifier;
    use zellij_tile::prelude::KeyWithModifier;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;
    use zellij_tile::prelude::TabInfo;

    use super::*;
    use crate::plugin::ppick::ui::PpickRow;
    use crate::plugin::tbar::StateSnapshotPayload;

    fn key(bare_key: BareKey) -> KeyWithModifier {
        KeyWithModifier::new(bare_key)
    }

    fn terminal_pane_with_command(id: u32, command: &str) -> PaneInfo {
        PaneInfo {
            id,
            terminal_command: Some(command.to_string()),
            ..Default::default()
        }
    }

    fn update_panes(
        state: &mut PpickState,
        manifest: &PaneManifest,
        resolve_pane_cwd: impl FnMut(u32) -> Option<PathBuf>,
        resolve_pane_command: impl FnMut(u32) -> Option<Vec<String>>,
    ) -> bool {
        state.update_panes(manifest, resolve_pane_cwd, resolve_pane_command)
    }

    fn frame(state: &mut PpickState) -> Vec<PpickRow> {
        state.mark_filter_dirty();
        state.visible_frame(usize::MAX)
    }

    fn session_entry(agent: &str, workspace: &str, session_id: &str, search: &str, updated_at: &str) -> SessionEntry {
        SessionEntry {
            agent: agent.to_string(),
            workspace: PathBuf::from(workspace),
            session_id: session_id.to_string(),
            summary: Some(format!("{agent} summary")),
            display: format!("{agent} {workspace} {session_id}"),
            search: search.to_string(),
            updated_at: updated_at.to_string(),
        }
    }

    #[test]
    fn test_take_floating_coordinates_centers_inside_active_display_area() {
        let mut state = PpickState::default();
        state.set_floating_coordinates(
            Some(String::from("2")),
            Some(String::from("68%")),
            Some(String::from("45%")),
        );
        let _ = state.update_tabs(vec![TabInfo {
            active: true,
            display_area_rows: 100,
            display_area_columns: 320,
            ..Default::default()
        }]);

        pretty_assertions::assert_eq!(
            state.take_floating_coordinates(),
            FloatingPaneCoordinates::new(
                Some(String::from("51")),
                Some(String::from("2")),
                Some(String::from("217")),
                Some(String::from("45")),
                None,
                Some(false),
            )
        );
        pretty_assertions::assert_eq!(state.take_floating_coordinates(), None);
    }

    #[test]
    fn test_take_floating_coordinates_reapplies_after_display_area_changes() {
        let mut state = PpickState::default();
        state.set_floating_coordinates(
            Some(String::from("0")),
            Some(String::from("68%")),
            Some(String::from("45%")),
        );
        let _ = state.update_tabs(vec![TabInfo {
            active: true,
            display_area_rows: 100,
            display_area_columns: 320,
            ..Default::default()
        }]);
        let _ = state.take_floating_coordinates();

        let _ = state.update_tabs(vec![TabInfo {
            active: true,
            display_area_rows: 100,
            display_area_columns: 200,
            ..Default::default()
        }]);

        pretty_assertions::assert_eq!(
            state.take_floating_coordinates(),
            FloatingPaneCoordinates::new(
                Some(String::from("32")),
                Some(String::from("0")),
                Some(String::from("136")),
                Some(String::from("45")),
                None,
                Some(false),
            )
        );
    }

    #[test]
    fn test_update_panes_includes_open_terminal_panes_and_excludes_plugins_and_suppressed_panes() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                name: "first".to_string(),
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                name: "second".to_string(),
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: [
                (
                    0,
                    vec![
                        PaneInfo {
                            id: 7,
                            is_plugin: true,
                            ..Default::default()
                        },
                        PaneInfo {
                            id: 9,
                            is_suppressed: true,
                            ..terminal_pane_with_command(9, "zsh")
                        },
                        PaneInfo {
                            id: 11,
                            exited: true,
                            ..terminal_pane_with_command(11, "nvim")
                        },
                        terminal_pane_with_command(10, "cargo"),
                    ],
                ),
                (
                    1,
                    vec![
                        PaneInfo {
                            id: 21,
                            is_held: true,
                            ..terminal_pane_with_command(21, "less")
                        },
                        terminal_pane_with_command(20, "codex"),
                    ],
                ),
            ]
            .into_iter()
            .collect(),
        };

        assert2::assert!(update_panes(
            &mut state,
            &manifest,
            |pane_id| Some(PathBuf::from(format!("/tmp/pane-{pane_id}"))),
            |pane_id| Some(vec![format!("cmd-{pane_id}")]),
        ));

        let pane_ids = state.pane_entries.iter().map(|entry| entry.pane_id).collect::<Vec<_>>();
        pretty_assertions::assert_eq!(pane_ids, vec![10, 11, 20, 21]);
        pretty_assertions::assert_eq!(
            frame(&mut state)
                .iter()
                .map(|row| row.cwd_label.as_str())
                .collect::<Vec<_>>(),
            vec!["/tmp/pane-10", "/tmp/pane-11", "/tmp/pane-20", "/tmp/pane-21",]
        );
    }

    #[test]
    fn test_update_panes_selects_initial_focused_pane_from_manifest() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    PaneInfo {
                        is_focused: true,
                        ..terminal_pane_with_command(43, "nvim")
                    },
                ],
            ))
            .collect(),
        };

        assert2::assert!(update_panes(&mut state, &manifest, |_| None, |_| None));

        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_update_panes_selects_active_tab_when_focus_is_not_observed() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: [
                (0, vec![terminal_pane_with_command(42, "cargo")]),
                (1, vec![terminal_pane_with_command(43, "nvim")]),
            ]
            .into_iter()
            .collect(),
        };

        assert2::assert!(update_panes(&mut state, &manifest, |_| None, |_| None));

        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_set_initial_focus_pane_selects_matching_pane_when_entries_arrive() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }]);
        assert2::assert!(!state.set_initial_focus_pane(10, 43, 1));
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    terminal_pane_with_command(43, "nvim"),
                ],
            ))
            .collect(),
        };

        assert2::assert!(update_panes(&mut state, &manifest, |_| None, |_| None));

        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_initial_focus_snapshot_ignores_older_snapshot_for_same_tab() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    terminal_pane_with_command(43, "nvim"),
                ],
            ))
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        assert2::assert!(state.set_initial_focus_pane(10, 43, 2));
        assert2::assert!(!state.set_initial_focus_pane(10, 42, 1));

        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_initial_focus_snapshot_accepts_newer_snapshot_for_same_tab() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    terminal_pane_with_command(43, "nvim"),
                ],
            ))
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        assert2::assert!(!state.set_initial_focus_pane(10, 42, 1));
        assert2::assert!(state.set_initial_focus_pane(10, 43, 2));

        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_initial_focus_selection_does_not_override_user_selection() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            position: 0,
            active: true,
            ..Default::default()
        }]);
        let _ = state.set_initial_focus_pane(10, 42, 1);
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "cargo"),
                    terminal_pane_with_command(43, "nvim"),
                ],
            ))
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);
        let ctrl_n = KeyWithModifier::new_with_modifiers(BareKey::Char('n'), BTreeSet::from([KeyModifier::Ctrl]));

        pretty_assertions::assert_eq!(state.selected, 0);
        pretty_assertions::assert_eq!(state.handle_key(&ctrl_n), PpickAction::Redraw);
        pretty_assertions::assert_eq!(state.selected, 1);
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        pretty_assertions::assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_initial_focus_snapshot_waits_for_active_tab_metadata() {
        let mut state = PpickState::default();
        assert2::assert!(!state.set_initial_focus_pane(20, 43, 1));
        let manifest = PaneManifest {
            panes: [
                (0, vec![terminal_pane_with_command(42, "cargo")]),
                (1, vec![terminal_pane_with_command(43, "nvim")]),
            ]
            .into_iter()
            .collect(),
        };

        assert2::assert!(update_panes(&mut state, &manifest, |_| None, |_| None));
        pretty_assertions::assert_eq!(state.selected, 0);
        assert2::assert!(state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                active: true,
                ..Default::default()
            },
        ]));

        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(frame(&mut state).get(1).map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_initial_focus_snapshot_from_inactive_tab_does_not_select_pane() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                active: true,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: [
                (0, vec![terminal_pane_with_command(42, "cargo")]),
                (1, vec![terminal_pane_with_command(43, "nvim")]),
            ]
            .into_iter()
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        assert2::assert!(!state.set_initial_focus_pane(20, 43, 1));

        pretty_assertions::assert_eq!(state.selected, 0);
        pretty_assertions::assert_eq!(frame(&mut state).first().map(|row| row.selected), Some(true));
    }

    #[test]
    fn test_update_tabs_orders_panes_by_tab_order_then_pane_id() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 30,
                position: 2,
                ..Default::default()
            },
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
        ]);
        state.pane_entries = vec![
            PaneEntry::new(2, 30, None, vec![String::from("third")], None),
            PaneEntry::new(0, 11, None, vec![String::from("first-b")], None),
            PaneEntry::new(0, 10, None, vec![String::from("first-a")], None),
        ];

        assert2::assert!(state.update_tabs(state.all_tabs.clone()));

        let pane_ids = state.pane_entries.iter().map(|entry| entry.pane_id).collect::<Vec<_>>();
        pretty_assertions::assert_eq!(pane_ids, vec![10, 11, 30]);
    }

    #[test]
    fn test_update_tabs_keeps_selected_pane_after_tab_order_changes() {
        let mut state = PpickState {
            pane_entries: vec![
                PaneEntry::new(0, 42, None, vec![String::from("cargo")], None),
                PaneEntry::new(1, 43, None, vec![String::from("nvim")], None),
            ],
            ..Default::default()
        };
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                ..Default::default()
            },
        ]);
        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Down)), PpickAction::Redraw);

        assert2::assert!(state.update_tabs(vec![
            TabInfo {
                tab_id: 20,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 10,
                position: 1,
                ..Default::default()
            },
        ]));

        pretty_assertions::assert_eq!(state.selected_pane_id, Some(43));
        pretty_assertions::assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_update_panes_keeps_stable_tab_id_when_panes_refresh_before_tabs_after_tab_move() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
                position: 1,
                ..Default::default()
            },
        ]);
        let manifest = PaneManifest {
            panes: std::iter::once((1, vec![terminal_pane_with_command(43, "nvim")])).collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);
        let rows = frame(&mut state);
        pretty_assertions::assert_eq!(rows.first().map(|row| row.pane_label.as_str()), Some("20:43"));

        let stale_tabs_manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(43, "nvim")])).collect(),
        };
        let _ = update_panes(&mut state, &stale_tabs_manifest, |_| None, |_| None);
        let rows = frame(&mut state);
        pretty_assertions::assert_eq!(rows.first().map(|row| row.pane_label.as_str()), Some("20:43"));

        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 20,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 10,
                position: 1,
                ..Default::default()
            },
        ]);

        let rows = frame(&mut state);
        pretty_assertions::assert_eq!(rows.first().map(|row| row.pane_label.as_str()), Some("20:43"));
    }

    #[test]
    fn test_visible_frame_shows_compact_pane_label_for_each_entry() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![
            TabInfo {
                tab_id: 10,
                position: 0,
                ..Default::default()
            },
            TabInfo {
                tab_id: 20,
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
                (1, vec![terminal_pane_with_command(44, "git")]),
            ]
            .into_iter()
            .collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| None);

        let rows = frame(&mut state);
        let labels = rows.iter().map(|row| row.pane_label.as_str()).collect::<Vec<_>>();

        pretty_assertions::assert_eq!(labels, vec!["10:42", "10:43", "20:44"]);
    }

    #[test]
    fn test_agent_only_mode_filters_non_agent_panes_and_keeps_seen_busy_and_unseen_agents() {
        let mut busy = PaneEntry::new(0, 44, None, vec![String::from("claude")], None);
        let _ = busy.apply_agent_snapshot(PaneAgentSnapshot {
            pane_id: 44,
            agent: Agent::Claude,
            indicator: agg::TabIndicator::Busy,
        });
        let mut unseen = PaneEntry::new(0, 45, None, vec![String::from("cursor-agent")], None);
        let _ = unseen.apply_agent_snapshot(PaneAgentSnapshot {
            pane_id: 45,
            agent: Agent::Cursor,
            indicator: agg::TabIndicator::Unseen,
        });
        let mut state = PpickState {
            pane_entries: vec![
                PaneEntry::new(0, 42, None, vec![String::from("cargo")], None),
                PaneEntry::new(0, 43, None, vec![String::from("codex")], None),
                busy,
                unseen,
            ],
            ..PpickState::new(PpickMode::AgentsOnly)
        };

        let rows = frame(&mut state);
        let labels = rows.iter().map(|row| row.pane_label.as_str()).collect::<Vec<_>>();
        let indicators = rows.iter().map(|row| row.indicator).collect::<Vec<_>>();

        pretty_assertions::assert_eq!(labels, vec!["43", "44", "45"]);
        pretty_assertions::assert_eq!(
            indicators,
            vec![
                agg::TabIndicator::Seen,
                agg::TabIndicator::Busy,
                agg::TabIndicator::Unseen,
            ]
        );
    }

    #[test]
    fn test_agent_only_mode_closes_empty_picker_only_without_query() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(0, 42, None, vec![String::from("cargo")], None)],
            ..PpickState::new(PpickMode::AgentsOnly)
        };

        assert2::assert!(state.should_close_empty_picker());

        state.query = String::from("codex");
        state.mark_filter_dirty();
        assert2::assert!(!state.should_close_empty_picker());
    }

    #[test]
    fn test_all_panes_mode_does_not_close_empty_picker() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(0, 42, None, vec![String::from("cargo")], None)],
            ..Default::default()
        };

        assert2::assert!(!state.should_close_empty_picker());
    }

    #[test]
    fn test_search_matches_pane_path_command_and_agent_label() {
        let entry = PaneEntry::new(
            0,
            42,
            Some(PathBuf::from("/Users/me/project")),
            vec![String::from("codex"), String::from("resume"), String::from("abc")],
            None,
        );

        for query in ["42", "project", "codex", "cx"] {
            assert2::assert!(entry.matches_normalized_query(query));
        }
    }

    #[test]
    fn test_handle_key_selection_uses_cached_filter_for_large_result() {
        let mut state = PpickState {
            pane_entries: (0..1_000)
                .map(|idx| {
                    PaneEntry::new(
                        0,
                        idx,
                        Some(PathBuf::from(format!("/tmp/work-{idx}"))),
                        vec![String::from("cargo")],
                        None,
                    )
                })
                .collect(),
            ..Default::default()
        };
        for c in "work".chars() {
            pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Char(c))), PpickAction::Redraw);
        }
        pretty_assertions::assert_eq!(state.filtered_entry_indices.len(), 1_000);
        let filtered_entry_indices = state.filtered_entry_indices.clone();

        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Down)), PpickAction::Redraw);

        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(state.filtered_entry_indices, filtered_entry_indices);
        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Enter)), PpickAction::Focus(1));
    }

    #[test]
    fn test_visible_frame_materializes_only_capacity_rows_and_keeps_selection_visible() {
        let mut state = PpickState {
            pane_entries: (0..8)
                .map(|idx| {
                    PaneEntry::new(
                        0,
                        idx,
                        Some(PathBuf::from(format!("/tmp/pane-{idx}"))),
                        vec![String::from("cargo")],
                        None,
                    )
                })
                .collect(),
            ..Default::default()
        };
        for _ in 0..5 {
            pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Down)), PpickAction::Redraw);
        }

        let frame = state.visible_frame(2);

        pretty_assertions::assert_eq!(frame.len(), 2);
        pretty_assertions::assert_eq!(
            frame.iter().map(|row| row.cwd_label.as_str()).collect::<Vec<_>>(),
            vec!["/tmp/pane-4", "/tmp/pane-5"]
        );
        pretty_assertions::assert_eq!(
            frame.iter().map(|row| row.selected).collect::<Vec<_>>(),
            vec![false, true]
        );
    }

    #[test]
    fn test_pane_update_resolves_running_command_only_for_uncached_panes() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![
                    terminal_pane_with_command(42, "zsh"),
                    terminal_pane_with_command(43, "zsh"),
                ],
            ))
            .collect(),
        };
        let command_calls = Cell::new(0);

        let _ = update_panes(
            &mut state,
            &manifest,
            |pane_id| Some(PathBuf::from(format!("/tmp/pane-{pane_id}"))),
            |pane_id| {
                command_calls.set(command_calls.get() + 1);
                Some(vec![format!("cmd-{pane_id}")])
            },
        );
        let _ = update_panes(
            &mut state,
            &manifest,
            |pane_id| Some(PathBuf::from(format!("/tmp/changed-{pane_id}"))),
            |pane_id| {
                command_calls.set(command_calls.get() + 1);
                Some(vec![format!("changed-{pane_id}")])
            },
        );

        pretty_assertions::assert_eq!(command_calls.get(), 2);
        pretty_assertions::assert_eq!(
            state
                .pane_entries
                .iter()
                .map(|entry| entry.command_args.clone())
                .collect::<Vec<_>>(),
            vec![vec![String::from("cmd-42")], vec![String::from("cmd-43")]]
        );
    }

    #[test]
    fn test_git_stat_refreshes_first_seen_and_cwd_changes_not_command_changes() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "zsh")])).collect(),
        };

        let _ = update_panes(
            &mut state,
            &manifest,
            |_| Some(PathBuf::from("/tmp/repo")),
            |_| Some(vec![String::from("cargo")]),
        );
        pretty_assertions::assert_eq!(state.take_git_stat_cwds_to_request(), vec![PathBuf::from("/tmp/repo")]);

        assert2::assert!(state.update_command(42, &[String::from("nvim")]));
        pretty_assertions::assert_eq!(state.take_git_stat_cwds_to_request(), Vec::<PathBuf>::new());

        assert2::assert!(state.update_cwd(42, &PathBuf::from("/tmp/other")));
        pretty_assertions::assert_eq!(state.take_git_stat_cwds_to_request(), vec![PathBuf::from("/tmp/other")]);
    }

    #[test]
    fn test_git_stat_requests_dedupe_while_in_flight() {
        let mut state = PpickState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "zsh")])).collect(),
        };
        let repo = PathBuf::from("/tmp/repo");
        let other = PathBuf::from("/tmp/other");
        let _ = update_panes(
            &mut state,
            &manifest,
            |_| Some(repo.clone()),
            |_| Some(vec![String::from("cargo")]),
        );

        pretty_assertions::assert_eq!(state.take_git_stat_cwds_to_request(), vec![repo.clone()]);
        assert2::assert!(state.update_cwd(42, &other));
        pretty_assertions::assert_eq!(state.take_git_stat_cwds_to_request(), vec![other.clone()]);
        assert2::assert!(state.update_cwd(42, &repo));
        pretty_assertions::assert_eq!(state.take_git_stat_cwds_to_request(), Vec::<PathBuf>::new());

        state.finish_git_stat_request(&repo);
        assert2::assert!(state.update_cwd(42, &other));
        pretty_assertions::assert_eq!(state.take_git_stat_cwds_to_request(), Vec::<PathBuf>::new());
        state.finish_git_stat_request(&other);
        assert2::assert!(state.update_cwd(42, &repo));
        pretty_assertions::assert_eq!(state.take_git_stat_cwds_to_request(), vec![repo]);
    }

    #[test]
    fn test_search_matches_attached_ags_hidden_session_text() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(
                0,
                42,
                Some(PathBuf::from("/tmp/repo")),
                vec![String::from("codex"), String::from("resume"), String::from("older")],
                None,
            )],
            ..Default::default()
        };

        assert2::assert!(state.update_sessions(vec![session_entry(
            "codex",
            "/tmp/repo",
            "older",
            "hidden prompt about billing",
            "2026-05-09T09:00:00Z",
        )]));

        state.query = String::from("BILLING");
        let frame = frame(&mut state);
        pretty_assertions::assert_eq!(frame.len(), 1);
    }

    #[test]
    fn test_frame_carries_attached_ags_session_summary() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(
                0,
                42,
                Some(PathBuf::from("/tmp/repo")),
                vec![
                    String::from("codex"),
                    String::from("resume"),
                    String::from("session-id"),
                ],
                None,
            )],
            ..Default::default()
        };
        let mut session = session_entry(
            "codex",
            "/tmp/repo",
            "session-id",
            "hidden prompt",
            "2026-05-09T09:00:00Z",
        );
        session.summary = Some(String::from("how to solve this warning"));

        let _ = state.update_sessions(vec![session]);

        let frame = frame(&mut state);
        pretty_assertions::assert_eq!(
            frame.first().map(|row| row.session_summary.as_str()),
            Some("how to solve this warning")
        );
    }

    #[test]
    fn test_git_stat_update_updates_matching_pane_frame() {
        let mut state = PpickState {
            pane_entries: vec![
                PaneEntry::new(
                    0,
                    42,
                    Some(PathBuf::from("/tmp/repo")),
                    vec![String::from("cargo")],
                    None,
                ),
                PaneEntry::new(
                    0,
                    43,
                    Some(PathBuf::from("/tmp/other")),
                    vec![String::from("nvim")],
                    None,
                ),
            ],
            ..Default::default()
        };
        let stat = agg::GitStat {
            path: PathBuf::from("/tmp/repo"),
            branch: Some("main".to_string()),
            insertions: 2,
            deletions: 1,
            new_files: 3,
            is_worktree: false,
            ..Default::default()
        };

        assert2::assert!(state.update_git_stat(&stat));

        let frame = frame(&mut state);
        pretty_assertions::assert_eq!(frame.first().map(|row| &row.git), Some(&stat));
        pretty_assertions::assert_eq!(frame.first().map(|row| row.branch_label.as_str()), Some("main"));
        pretty_assertions::assert_eq!(frame.get(1).map(|row| &row.git), Some(&agg::GitStat::default()));
    }

    #[test]
    fn test_agent_events_follow_tbar_marker_transitions() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            active: true,
            position: 0,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "codex")])).collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| Some(vec![String::from("codex")]));

        assert2::assert!(state.update_agent(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Busy,
        }));
        pretty_assertions::assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Busy)
        );

        assert2::assert!(state.update_agent(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        }));
        pretty_assertions::assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Unseen)
        );

        let focused_manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![PaneInfo {
                    is_focused: true,
                    ..terminal_pane_with_command(42, "codex")
                }],
            ))
            .collect(),
        };
        let _ = update_panes(
            &mut state,
            &focused_manifest,
            |_| None,
            |_| Some(vec![String::from("codex")]),
        );

        pretty_assertions::assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Seen)
        );
    }

    #[test]
    fn test_state_snapshot_before_pane_update_hydrates_busy_agent() {
        let mut state = PpickState::default();
        let snapshot = StateSnapshotPayload {
            tab_id: 10,
            seq: 1,
            focused_pane_id: Some(42),
            cwd: None,
            cmd: agg::Cmd::None,
            indicator: agg::TabIndicator::NoAgent,
            git_stat: agg::GitStat::default(),
            pane_agents: vec![PaneAgentSnapshot {
                pane_id: 42,
                agent: Agent::Codex,
                indicator: agg::TabIndicator::Busy,
            }],
        };
        let _ = state.apply_state_snapshot(&snapshot);
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            active: true,
            position: 0,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "codex")])).collect(),
        };

        let _ = update_panes(&mut state, &manifest, |_| None, |_| Some(vec![String::from("codex")]));

        pretty_assertions::assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Busy)
        );
    }

    #[test]
    fn test_state_snapshot_after_pane_update_replaces_seen_with_unseen() {
        let mut state = PpickState::default();
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            active: true,
            position: 0,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "codex")])).collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| Some(vec![String::from("codex")]));
        pretty_assertions::assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Seen)
        );

        let snapshot = StateSnapshotPayload {
            tab_id: 10,
            seq: 1,
            focused_pane_id: Some(42),
            cwd: None,
            cmd: agg::Cmd::None,
            indicator: agg::TabIndicator::NoAgent,
            git_stat: agg::GitStat::default(),
            pane_agents: vec![PaneAgentSnapshot {
                pane_id: 42,
                agent: Agent::Codex,
                indicator: agg::TabIndicator::Unseen,
            }],
        };

        assert2::assert!(state.apply_state_snapshot(&snapshot));

        pretty_assertions::assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Unseen)
        );
    }

    #[test]
    fn test_agent_event_invalidates_state_snapshot_cache() {
        let mut state = PpickState::default();
        let snapshot = StateSnapshotPayload {
            tab_id: 10,
            seq: 1,
            focused_pane_id: Some(42),
            cwd: None,
            cmd: agg::Cmd::None,
            indicator: agg::TabIndicator::NoAgent,
            git_stat: agg::GitStat::default(),
            pane_agents: vec![PaneAgentSnapshot {
                pane_id: 42,
                agent: Agent::Codex,
                indicator: agg::TabIndicator::Busy,
            }],
        };
        let _ = state.apply_state_snapshot(&snapshot);
        let _ = state.update_tabs(vec![TabInfo {
            tab_id: 10,
            active: true,
            position: 0,
            ..Default::default()
        }]);
        let manifest = PaneManifest {
            panes: std::iter::once((0, vec![terminal_pane_with_command(42, "codex")])).collect(),
        };
        let _ = update_panes(&mut state, &manifest, |_| None, |_| Some(vec![String::from("codex")]));

        pretty_assertions::assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Busy)
        );

        assert2::assert!(state.update_agent(&AgentEventPayload {
            pane_id: 42,
            agent: Agent::Codex,
            kind: AgentEventKind::Idle,
        }));

        let focused_manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![PaneInfo {
                    is_focused: true,
                    ..terminal_pane_with_command(42, "codex")
                }],
            ))
            .collect(),
        };
        let _ = update_panes(
            &mut state,
            &focused_manifest,
            |_| None,
            |_| Some(vec![String::from("codex")]),
        );

        pretty_assertions::assert_eq!(
            frame(&mut state).first().map(|row| row.indicator),
            Some(agg::TabIndicator::Seen)
        );
    }

    #[test]
    fn test_agent_pane_without_session_id_does_not_attach_ags_data() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(
                0,
                42,
                Some(PathBuf::from("/tmp/repo")),
                vec![String::from("codex")],
                None,
            )],
            query: String::from("repo"),
            ..Default::default()
        };

        assert2::assert!(state.update_sessions(vec![session_entry(
            "codex",
            "/tmp/repo",
            "session-id",
            "hidden",
            "2026-05-09T09:00:00Z",
        )]));

        pretty_assertions::assert_eq!(frame(&mut state).len(), 1);
        pretty_assertions::assert_eq!(
            state
                .pane_entries
                .first()
                .and_then(|entry| entry.session_search.as_deref()),
            None
        );
    }

    #[test]
    fn test_exact_session_id_match_attaches_ags_data() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(
                0,
                42,
                Some(PathBuf::from("/tmp/repo")),
                vec![String::from("codex"), String::from("resume"), String::from("exact")],
                None,
            )],
            ..Default::default()
        };

        let _ = state.update_sessions(vec![
            session_entry("codex", "/tmp/repo", "new", "new hidden", "2026-05-09T10:00:00Z"),
            session_entry("codex", "/tmp/repo", "exact", "exact hidden", "2026-05-09T09:00:00Z"),
        ]);

        pretty_assertions::assert_eq!(
            state
                .pane_entries
                .first()
                .and_then(|entry| entry.session_search.as_deref()),
            Some("exact hidden")
        );
    }

    #[test]
    fn test_handle_key_updates_query_backspace_esc_empty_and_enter() {
        let mut state = PpickState {
            pane_entries: vec![PaneEntry::new(0, 42, None, vec![String::from("cargo")], None)],
            ..Default::default()
        };

        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Char('c'))), PpickAction::Redraw);
        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Backspace)), PpickAction::Redraw);
        pretty_assertions::assert_eq!(state.query, "");
        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Esc)), PpickAction::Close);
        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Enter)), PpickAction::Focus(42));
        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Char('x'))), PpickAction::Redraw);
        pretty_assertions::assert_eq!(state.handle_key(&key(BareKey::Enter)), PpickAction::None);
    }

    #[test]
    fn test_handle_key_ctrl_n_and_ctrl_p_loop_selection() {
        let mut state = PpickState {
            pane_entries: vec![
                PaneEntry::new(0, 42, None, vec![String::from("cargo")], None),
                PaneEntry::new(0, 43, None, vec![String::from("nvim")], None),
            ],
            ..Default::default()
        };
        let ctrl_n = KeyWithModifier::new_with_modifiers(BareKey::Char('n'), BTreeSet::from([KeyModifier::Ctrl]));
        let ctrl_p = KeyWithModifier::new_with_modifiers(BareKey::Char('p'), BTreeSet::from([KeyModifier::Ctrl]));

        pretty_assertions::assert_eq!(state.handle_key(&ctrl_n), PpickAction::Redraw);
        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(state.handle_key(&ctrl_n), PpickAction::Redraw);
        pretty_assertions::assert_eq!(state.selected, 0);
        pretty_assertions::assert_eq!(state.handle_key(&ctrl_p), PpickAction::Redraw);
        pretty_assertions::assert_eq!(state.selected, 1);
        pretty_assertions::assert_eq!(state.handle_key(&ctrl_p), PpickAction::Redraw);
        pretty_assertions::assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_handle_key_selection_loop_is_noop_for_empty_or_single_result() {
        let ctrl_n = KeyWithModifier::new_with_modifiers(BareKey::Char('n'), BTreeSet::from([KeyModifier::Ctrl]));
        let ctrl_p = KeyWithModifier::new_with_modifiers(BareKey::Char('p'), BTreeSet::from([KeyModifier::Ctrl]));
        let mut empty_state = PpickState::default();
        let mut single_state = PpickState {
            pane_entries: vec![PaneEntry::new(0, 42, None, vec![String::from("cargo")], None)],
            ..Default::default()
        };

        pretty_assertions::assert_eq!(empty_state.handle_key(&ctrl_n), PpickAction::None);
        pretty_assertions::assert_eq!(empty_state.handle_key(&ctrl_p), PpickAction::None);
        pretty_assertions::assert_eq!(single_state.handle_key(&ctrl_n), PpickAction::None);
        pretty_assertions::assert_eq!(single_state.handle_key(&ctrl_p), PpickAction::None);
        pretty_assertions::assert_eq!(single_state.selected, 0);
    }
}
