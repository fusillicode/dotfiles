use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

use kanal::Sender;
use muxr_config::ScrollbackDumpStyle;
use muxr_core::PaneId;
use muxr_core::TerminalSize;
use muxr_core::TrackedProcessState;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::history::pane_output_path;
use crate::pane::layout::PaneRegion;
use crate::pane::tracked_process::PaneTrackedProcessSnapshot;
use crate::pty::PtyEvent;
use crate::pty::PtyExitStatus;
use crate::pty::PtyHandle;
use crate::pty::PtySession;
use crate::pty::PtySinkGuard;
use crate::pty::ShellCmd;
use crate::server::ServerConfig;
use crate::session::start_seed::SessionStartSeed;
use crate::state::PaneMetadataSync;
use crate::state::PaneSnapshotFields;
use crate::state::SessionLayout;
use crate::terminal::TerminalSnapshot;

struct PaneRuntime {
    id: PaneId,
    session: PtySession,
    startup_cmd_label: Option<String>,
}

/// Terminal-title sync result for layout metadata derived from live pane runtimes.
pub struct SyncedTerminalTitles {
    metadata_sync: PaneMetadataSync,
    titles: Vec<(PaneId, Option<String>)>,
}

impl SyncedTerminalTitles {
    pub const fn metadata_sync(&self) -> PaneMetadataSync {
        self.metadata_sync
    }

    /// Return the runtime terminal titles that were applied to the layout.
    pub fn titles(&self) -> &[(PaneId, Option<String>)] {
        &self.titles
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PaneRuntimeMetadataEntry {
    startup_cmd_label: Option<String>,
    terminal_title: Option<String>,
    tracked_cmd_label: Option<String>,
    tracked_process_state: TrackedProcessState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneRuntimeMetadata {
    panes: BTreeMap<PaneId, PaneRuntimeMetadataEntry>,
}

impl PaneRuntimeMetadata {
    pub fn from_sources(
        terminal_titles: Vec<(PaneId, Option<String>)>,
        startup_cmd_labels: Vec<(PaneId, Option<String>)>,
        tracked_processes: &PaneTrackedProcessSnapshot,
    ) -> Self {
        let mut panes = BTreeMap::<PaneId, PaneRuntimeMetadataEntry>::new();
        for (pane_id, terminal_title) in terminal_titles {
            panes.entry(pane_id).or_default().terminal_title = terminal_title;
        }
        for (pane_id, startup_cmd_label) in startup_cmd_labels {
            panes.entry(pane_id).or_default().startup_cmd_label = startup_cmd_label;
        }
        for (pane_id, tracked_process) in tracked_processes.panes() {
            let pane = panes.entry(pane_id).or_default();
            pane.tracked_cmd_label = Some(tracked_process.label().to_owned());
            pane.tracked_process_state = tracked_process.state();
        }
        Self { panes }
    }

    pub fn with_terminal_title_override(&self, pane_id: PaneId, terminal_title: Option<String>) -> Self {
        let mut out = self.clone();
        out.panes.entry(pane_id).or_default().terminal_title = terminal_title;
        out
    }

    pub fn pane_snapshot_fields(&self) -> PaneSnapshotFields {
        let mut fields = PaneSnapshotFields::default();
        for (pane_id, pane) in &self.panes {
            fields.set_terminal_title(*pane_id, pane.terminal_title.clone());
            fields.set_cmd_label(*pane_id, self::runtime_cmd_label(pane));
            fields.set_tracked_process_state(*pane_id, pane.tracked_process_state);
        }
        fields
    }
}

fn runtime_cmd_label(pane: &PaneRuntimeMetadataEntry) -> Option<String> {
    pane.tracked_cmd_label
        .as_ref()
        .or_else(|| {
            let has_terminal_title = pane
                .terminal_title
                .as_deref()
                .is_some_and(|title| !title.trim().is_empty());
            (!has_terminal_title)
                .then_some(pane.startup_cmd_label.as_ref())
                .flatten()
        })
        .cloned()
}

pub struct PaneRuntimes {
    pane_exit_notify: Arc<tokio::sync::Notify>,
    panes: Vec<PaneRuntime>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneRuntimeSetStatus {
    Empty,
    HasPanes,
}

impl PaneRuntimes {
    pub fn spawn_for_start_seed(
        config: &ServerConfig,
        start_seed: &SessionStartSeed,
        size: &TerminalSize,
        pane_exit_notify: Arc<tokio::sync::Notify>,
    ) -> rootcause::Result<Self> {
        let mut panes = Vec::new();
        for pane in start_seed.layout.panes() {
            let startup_cmd = start_seed
                .startup_cmds
                .iter()
                .find(|(startup_pane_id, _cmd)| *startup_pane_id == pane.id);
            let runtime = PaneRuntime {
                startup_cmd_label: startup_cmd.map(|(_pane_id, cmd)| cmd.label_with_args()),
                session: PtySession::spawn(
                    &config.shell_cmd,
                    &pane.cwd,
                    size,
                    &self::pane_output_path(&config.paths.panes, pane.id),
                    config.user_config.scrollback,
                    Arc::clone(&pane_exit_notify),
                )?,
                id: pane.id,
            };
            if let Some((_pane_id, cmd)) = startup_cmd {
                // External-layout commands are startup input, not the pane lifetime process. A one-shot command such
                // as `demo process start` may exit immediately, but the shell pane must remain part of the layout.
                let _scrolled = runtime
                    .session
                    .handle()
                    .write_input(cmd.shell_input_line().as_bytes())?;
            }
            panes.push(runtime);
        }
        Ok(Self {
            pane_exit_notify,
            panes,
        })
    }

    pub fn spawn_pane(
        &mut self,
        pane_id: PaneId,
        cwd: &str,
        config: &ServerConfig,
        size: &TerminalSize,
    ) -> rootcause::Result<()> {
        let history_path = self::pane_output_path(&config.paths.panes, pane_id);
        self.panes.push(PaneRuntime {
            id: pane_id,
            startup_cmd_label: None,
            session: PtySession::spawn(
                &config.shell_cmd,
                cwd,
                size,
                &history_path,
                config.user_config.scrollback,
                Arc::clone(&self.pane_exit_notify),
            )?,
        });
        Ok(())
    }

    pub fn spawn_cmd_pane(
        &mut self,
        pane_id: PaneId,
        cwd: &str,
        cmd: &ShellCmd,
        startup_cmd_label: Option<String>,
        config: &ServerConfig,
        size: &TerminalSize,
    ) -> rootcause::Result<()> {
        let history_path = self::pane_output_path(&config.paths.panes, pane_id);
        self.panes.push(PaneRuntime {
            id: pane_id,
            startup_cmd_label,
            session: PtySession::spawn(
                cmd,
                cwd,
                size,
                &history_path,
                config.user_config.scrollback,
                Arc::clone(&self.pane_exit_notify),
            )?,
        });
        Ok(())
    }

    pub fn handle(&self, pane_id: PaneId) -> rootcause::Result<PtyHandle> {
        self.panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .map(|pane| pane.session.handle())
            .ok_or_else(|| report!("muxr pane runtime is missing").attach(format!("pane_id={pane_id}")))
    }

    pub fn attach_sinks(&self, sender: &Sender<PtyEvent>) -> rootcause::Result<Vec<(PaneId, PtySinkGuard)>> {
        self.panes
            .iter()
            .map(|pane| Ok((pane.id, pane.session.handle().attach_sink(sender.clone())?)))
            .collect()
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.panes.iter().map(|pane| pane.id).collect()
    }

    pub fn startup_cmd_labels(&self) -> Vec<(PaneId, Option<String>)> {
        self.panes
            .iter()
            .filter_map(|pane| {
                pane.startup_cmd_label
                    .as_ref()
                    .map(|cmd_label| (pane.id, Some(cmd_label.clone())))
            })
            .collect()
    }

    pub fn remove(&mut self, pane_id: PaneId) {
        self.panes.retain(|pane| pane.id != pane_id);
    }

    pub const fn set_status(&self) -> PaneRuntimeSetStatus {
        if self.panes.is_empty() {
            PaneRuntimeSetStatus::Empty
        } else {
            PaneRuntimeSetStatus::HasPanes
        }
    }

    pub fn exited_panes(&self) -> rootcause::Result<Vec<(PaneId, PtyExitStatus)>> {
        let mut exited_panes = Vec::new();
        for pane in &self.panes {
            let handle = pane.session.handle();
            if handle.exit_state() == crate::pty::PtyExitState::Exited {
                let Some(exit_status) = handle.exit_status()? else {
                    return Err(
                        report!("muxr exited pane is missing exit status").attach(format!("pane_id={}", pane.id))
                    );
                };
                exited_panes.push((pane.id, exit_status));
            }
        }
        Ok(exited_panes)
    }

    pub fn resize_panes(&self, regions: &[PaneRegion]) -> rootcause::Result<()> {
        for region in regions {
            self.handle(region.id)?
                .resize(&TerminalSize::new(region.area.size.cols, region.area.size.rows)?)?;
        }
        Ok(())
    }

    pub fn snapshot(&self, pane_id: PaneId) -> rootcause::Result<TerminalSnapshot> {
        self.handle(pane_id)?.render_snapshot()
    }

    pub fn write_scrollback_dump(
        &self,
        pane_id: PaneId,
        style: ScrollbackDumpStyle,
        writer: &mut impl Write,
    ) -> rootcause::Result<()> {
        self.handle(pane_id)?.write_scrollback_dump(style, writer)
    }

    pub fn terminal_titles(&self) -> rootcause::Result<Vec<(PaneId, Option<String>)>> {
        self.panes
            .iter()
            .filter_map(|pane| match pane.session.handle().terminal_title() {
                Ok(Some(title)) => Some(Ok((pane.id, Some(title)))),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    /// Sync runtime terminal titles into layout metadata and return the applied titles.
    pub fn sync_layout_terminal_titles(&self, layout: &mut SessionLayout) -> rootcause::Result<SyncedTerminalTitles> {
        let terminal_titles = self.terminal_titles()?;
        // Shell prompts report cwd through OSC title updates. Keep layout metadata in sync before layout mutations so
        // new panes inherit the live cwd instead of the server startup directory.
        let metadata_sync = layout.sync_terminal_titles(&terminal_titles);
        Ok(SyncedTerminalTitles {
            metadata_sync,
            titles: terminal_titles,
        })
    }

    pub fn take_title_changes(&self) -> rootcause::Result<Vec<(PaneId, Option<String>)>> {
        let mut title_changes = Vec::new();
        for pane in &self.panes {
            for title in pane.session.handle().take_title_changes()? {
                title_changes.push((pane.id, title));
            }
        }
        Ok(title_changes)
    }

    pub fn take_screen_dirty_panes(&self) -> Vec<PaneId> {
        let mut screen_dirty_panes = Vec::new();
        for pane in &self.panes {
            if pane.session.handle().take_screen_dirty() == crate::pty::PtyScreenDmg::Dirty {
                screen_dirty_panes.push(pane.id);
            }
        }
        screen_dirty_panes
    }
}

/// Apply live runtime terminal titles to layout metadata before layout mutations.
pub fn sync_layout_terminal_titles(layout: &mut SessionLayout, runtimes: &PaneRuntimes) -> rootcause::Result<()> {
    let _ = runtimes.sync_layout_terminal_titles(layout)?;
    Ok(())
}

pub fn spawn_pane_or_restore_layout(
    layout: &mut SessionLayout,
    previous_layout: SessionLayout,
    pane_id: PaneId,
    config: &ServerConfig,
    runtimes: &mut PaneRuntimes,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneId> {
    // New panes update layout and runtimes together; rollback the layout if PTY spawn fails so render cannot see
    // pane metadata without a runtime.
    let cwd = layout
        .pane(pane_id)
        .map(|pane| pane.cwd.clone())
        .ok_or_else(|| report!("muxr new pane is missing from server layout").attach(format!("pane_id={pane_id}")))?;
    let spawn_result = runtimes.spawn_pane(pane_id, &cwd, config, terminal_size);
    if let Err(error) = spawn_result {
        *layout = previous_layout;
        return Err(error).attach("rolled back muxr layout after pane spawn failure");
    }
    Ok(pane_id)
}

#[cfg(test)]
pub mod test_helpers {
    use super::*;

    pub fn empty_runtimes() -> PaneRuntimes {
        PaneRuntimes {
            pane_exit_notify: Arc::new(tokio::sync::Notify::new()),
            panes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use muxr_config::MuxrConfig;
    use muxr_core::PaneId;
    use muxr_core::TerminalSize;
    use rootcause::report;
    use test_that::prelude::*;

    use super::*;
    use crate::pane::cmd::PaneCmd;
    use crate::pane::cmd::PaneCmdObservation;
    use crate::pane::tracked_process::PaneTrackedProcesses;
    use crate::pty::ShellCmd;
    use crate::server::test_helpers as server_test_helpers;
    use crate::state::SessionLayout;
    use crate::state::SessionMetadata;
    use crate::terminal::TerminalSnapshot;

    #[test]
    fn test_spawn_for_start_seed_when_pane_cmd_exists_runs_cmd_inside_shell() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        config.shell_cmd = server_test_helpers::shell_cmd("/bin/sh");
        let layout = SessionLayout::initial(
            &config.session,
            SessionMetadata {
                cmd_label: "sh".to_owned(),
                cwd: tempdir.path().to_string_lossy().into_owned(),
                started_at: 1,
            },
        )?;
        let pane_id = PaneId::new(1)?;
        let start_seed = SessionStartSeed {
            layout,
            startup_cmds: vec![(pane_id, ShellCmd::with_args("/bin/echo", ["seeded"])?)],
        };
        let runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &start_seed,
            &TerminalSize::new(80, 24)?,
            Arc::new(tokio::sync::Notify::new()),
        )?;

        assert_that!(
            runtimes.startup_cmd_labels(),
            eq(vec![(pane_id, Some("echo seeded".to_owned()))])
        );
        self::wait_for_runtime_snapshot_contains(&runtimes, pane_id, "seeded")?;
        assert_that!(runtimes.exited_panes()?, eq(Vec::new()));
        Ok(())
    }

    #[test]
    fn test_pane_runtime_metadata_cmd_labels_when_title_or_tracked_label_exists_suppresses_startup_label()
    -> rootcause::Result<()> {
        let pane_1 = PaneId::new(1)?;
        let pane_2 = PaneId::new(2)?;
        let pane_3 = PaneId::new(3)?;
        let layout = SessionLayout::initial(
            &"work".parse()?,
            SessionMetadata {
                cmd_label: "sh".to_owned(),
                cwd: "/tmp".to_owned(),
                started_at: 1,
            },
        )?;
        let mut tracked_processes = PaneTrackedProcesses::default();
        assert_that!(
            tracked_processes
                .observe_pane_cmd(
                    &MuxrConfig::default(),
                    pane_1,
                    &PaneCmdObservation::FgCmd(crate::pane::cmd::FgCmd::from_test_cmd(PaneCmd {
                        executable: "codex".to_owned(),
                        path: None,
                        pid: 42,
                    })),
                    Instant::now(),
                )
                .state_change()
                == crate::pane::tracked_process::TrackedProcessStateChange::Changed,
            eq(true)
        );

        let metadata = PaneRuntimeMetadata::from_sources(
            vec![(pane_2, Some("~/work".to_owned()))],
            vec![
                (pane_1, Some("codex".to_owned())),
                (pane_2, Some("demo process start".to_owned())),
                (pane_3, Some("echo seeded".to_owned())),
            ],
            &tracked_processes.snapshot(&layout),
        );
        let snapshot_fields = metadata.pane_snapshot_fields();

        assert_that!(snapshot_fields.cmd_label(pane_1), eq(Some("cx")));
        assert_that!(snapshot_fields.cmd_label(pane_2), eq(None));
        assert_that!(snapshot_fields.cmd_label(pane_3), eq(Some("echo seeded")));
        assert_that!(snapshot_fields.terminal_title(pane_2), eq(Some("~/work")));
        assert_that!(
            snapshot_fields.tracked_process_state(pane_1),
            eq(TrackedProcessState::Busy)
        );
        Ok(())
    }

    fn wait_for_runtime_snapshot_contains(
        runtimes: &PaneRuntimes,
        pane_id: PaneId,
        needle: &str,
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            let snapshot = runtimes.handle(pane_id)?.render_snapshot()?;
            if self::snapshot_text(&snapshot).contains(needle) {
                return Ok(());
            }
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(report!("timed out waiting for muxr runtime snapshot").attach(format!("needle={needle}")));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn snapshot_text(snapshot: &TerminalSnapshot) -> String {
        snapshot
            .rows()
            .iter()
            .flat_map(muxr_core::RenderRowSpan::cells)
            .map(muxr_core::RenderCell::text)
            .collect()
    }
}
