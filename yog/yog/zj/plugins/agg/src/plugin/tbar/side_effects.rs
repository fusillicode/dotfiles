use std::collections::BTreeSet;

use crate::plugin::tbar::Event;
use crate::plugin::tbar::TbarState;

// Discrete IO actions tbar may perform after applying an event batch. Keeping
// them explicit makes the git-stat and snapshot behavior visible at the call
// site instead of hiding it behind independent flags.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum SideEffect {
    RunCurrentTabGitStat,
    SendCurrentTabSnapshot,
    SendSyncRequest,
}

// Convert an applied event batch into the IO work it requires. Tab activation
// often arrives as several events; deduping here prevents duplicate git-stat
// process launches and snapshot broadcasts while preserving the rule that
// git-stat follows the displayed pane/cwd.
pub(super) fn derive(state: &TbarState, events: &[Event]) -> Vec<SideEffect> {
    let mut side_effects = BTreeSet::new();
    let mut refresh_if_stale = false;
    for event in events {
        match event {
            Event::AgentIdle { .. } | Event::FocusChanged { .. } | Event::CwdChanged { .. } => {
                // These events can directly change the displayed cwd or agent
                // winner, so they force exactly one refresh for the post-apply
                // display source.
                side_effects.insert(SideEffect::RunCurrentTabGitStat);
                side_effects.insert(SideEffect::SendCurrentTabSnapshot);
            }
            Event::TabCreated { .. }
            | Event::TabRemapped { .. }
            | Event::AgentDetected { .. }
            | Event::AgentBusy { .. }
            | Event::AgentLost { .. }
            | Event::ActiveTabChanged { .. }
            | Event::BecameActive => {
                // These events can expose a different displayed source, but
                // only need git-stat IO when the cached stat no longer matches
                // that source.
                refresh_if_stale = true;
                side_effects.insert(SideEffect::SendCurrentTabSnapshot);
            }
            // A git-stat result already refreshes current state; snapshot it,
            // but do not enqueue another run and create a self-refresh loop.
            Event::GitStatChanged { .. } => {
                side_effects.insert(SideEffect::SendCurrentTabSnapshot);
            }
            Event::SyncRequested => {
                side_effects.insert(SideEffect::SendSyncRequest);
            }
            Event::PanesChanged { .. }
            | Event::RemoteTabUpdated { .. }
            | Event::TopologyChanged
            | Event::AllTabsReplaced { .. } => {}
        }
    }

    if refresh_if_stale && display_git_stat_is_stale(state) {
        side_effects.insert(SideEffect::RunCurrentTabGitStat);
    }
    side_effects.into_iter().collect()
}

fn display_git_stat_is_stale(state: &TbarState) -> bool {
    let Some(current_tab) = state.current_tab.as_ref() else {
        return false;
    };
    // Staleness is measured against the row display source, not the last
    // focused pane. That keeps unfocused agent tabs on the agent cwd and stat.
    let display = current_tab.current_row_display_source(state.current_tab_is_active(), &state.cwds_by_pane);
    display
        .cwd
        .as_ref()
        .is_some_and(|cwd| current_tab.git_stat.path != *cwd)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use agg::GitStat;
    use ytil_agents::agent::Agent;
    use zellij_tile::prelude::TabInfo;

    use super::*;
    use crate::plugin::pane::FocusedPane;
    use crate::plugin::tbar::Event;
    use crate::plugin::tbar::TbarState;
    use crate::plugin::tbar::current_tab::CurrentTab;

    #[test]
    fn test_derive_batches_activation_refresh_and_snapshot() {
        let state = state_with_display_git_stat("/Users/me/project", "/Users/me/project");

        pretty_assertions::assert_eq!(
            derive(
                &state,
                &[
                    Event::ActiveTabChanged { active_tab_id: 10 },
                    Event::BecameActive,
                    Event::FocusChanged {
                        new_pane: Some(FocusedPane { id: 42, label: None }),
                        acknowledge_existing_attention: true,
                    },
                ],
            ),
            vec![SideEffect::RunCurrentTabGitStat, SideEffect::SendCurrentTabSnapshot]
        );
    }

    #[test]
    fn test_derive_skips_nonforced_fresh_git_stat_refresh() {
        let state = state_with_display_git_stat("/Users/me/project", "/Users/me/project");

        pretty_assertions::assert_eq!(
            derive(
                &state,
                &[Event::AgentBusy {
                    pane_id: 42,
                    agent: Agent::Codex,
                }],
            ),
            vec![SideEffect::SendCurrentTabSnapshot]
        );
    }

    #[test]
    fn test_derive_refreshes_nonforced_stale_git_stat() {
        let state = state_with_display_git_stat("/Users/me/project", "/Users/me/other");

        pretty_assertions::assert_eq!(
            derive(&state, &[Event::BecameActive]),
            vec![SideEffect::RunCurrentTabGitStat, SideEffect::SendCurrentTabSnapshot]
        );
    }

    #[test]
    fn test_derive_git_stat_changed_never_self_refreshes() {
        let state = state_with_display_git_stat("/Users/me/project", "/Users/me/other");

        pretty_assertions::assert_eq!(
            derive(
                &state,
                &[Event::GitStatChanged {
                    new_stat: GitStat {
                        path: PathBuf::from("/Users/me/project"),
                        insertions: 1,
                        ..GitStat::default()
                    },
                }],
            ),
            vec![SideEffect::SendCurrentTabSnapshot]
        );
    }

    #[test]
    fn test_derive_keeps_sync_request_explicit() {
        let state = state_with_display_git_stat("/Users/me/project", "/Users/me/project");

        pretty_assertions::assert_eq!(
            derive(&state, &[Event::SyncRequested]),
            vec![SideEffect::SendSyncRequest]
        );
    }

    fn state_with_display_git_stat(display_cwd: &str, git_stat_path: &str) -> TbarState {
        TbarState {
            known_active_tab_id: Some(10),
            all_tabs: vec![TabInfo {
                tab_id: 10,
                active: true,
                ..Default::default()
            }],
            current_tab: Some(CurrentTab {
                pane_ids: std::iter::once(42).collect(),
                focused_pane: Some(FocusedPane { id: 42, label: None }),
                active_focus_pane_id: Some(42),
                git_stat: GitStat {
                    path: PathBuf::from(git_stat_path),
                    ..GitStat::default()
                },
                ..CurrentTab::new(10)
            }),
            cwds_by_pane: HashMap::from([(42, PathBuf::from(display_cwd))]),
            ..Default::default()
        }
    }
}
