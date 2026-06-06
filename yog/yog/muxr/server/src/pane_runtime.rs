use std::sync::Mutex;
use std::sync::mpsc;

use muxr_core::PaneId;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::history::pane_output_path;
use crate::pane_layout::PaneRegion;
use crate::pty::PtyEvent;
use crate::pty::PtyExitStatus;
use crate::pty::PtyHandle;
use crate::pty::PtySession;
use crate::pty::PtySinkGuard;
use crate::server::ServerConfig;
use crate::state::SessionLayout;
use crate::terminal::TerminalSnapshot;

struct PaneRuntime {
    id: PaneId,
    session: PtySession,
}

/// Terminal-title sync result for layout metadata derived from live pane runtimes.
pub struct SyncedTerminalTitles {
    layout_changed: bool,
    titles: Vec<(PaneId, Option<String>)>,
}

impl SyncedTerminalTitles {
    /// Return whether applying the runtime titles changed persisted layout metadata.
    pub const fn layout_changed(&self) -> bool {
        self.layout_changed
    }

    /// Return the runtime terminal titles that were applied to the layout.
    pub fn titles(&self) -> &[(PaneId, Option<String>)] {
        &self.titles
    }
}

pub struct PaneRuntimes {
    panes: Vec<PaneRuntime>,
}

impl PaneRuntimes {
    pub fn spawn_for_layout(
        config: &ServerConfig,
        layout: &SessionLayout,
        size: &TerminalSize,
    ) -> rootcause::Result<Self> {
        let mut panes = Vec::new();
        for pane in layout.panes() {
            panes.push(PaneRuntime {
                session: PtySession::spawn(
                    &config.shell_cmd,
                    &pane.cwd,
                    size,
                    &self::pane_output_path(&config.paths.panes, pane.id),
                )?,
                id: pane.id,
            });
        }
        Ok(Self { panes })
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
            session: PtySession::spawn(&config.shell_cmd, cwd, size, &history_path)?,
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

    pub fn attach_sinks(&self, sender: &mpsc::SyncSender<PtyEvent>) -> rootcause::Result<Vec<(PaneId, PtySinkGuard)>> {
        self.panes
            .iter()
            .map(|pane| Ok((pane.id, pane.session.handle().attach_sink(sender.clone())?)))
            .collect()
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.panes.iter().map(|pane| pane.id).collect()
    }

    pub fn remove(&mut self, pane_id: PaneId) {
        self.panes.retain(|pane| pane.id != pane_id);
    }

    pub const fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    pub fn exited_panes(&self) -> rootcause::Result<Vec<(PaneId, PtyExitStatus)>> {
        let mut exited_panes = Vec::new();
        for pane in &self.panes {
            let handle = pane.session.handle();
            if handle.has_exited()? {
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
        let layout_changed = layout.sync_terminal_titles(&terminal_titles);
        Ok(SyncedTerminalTitles {
            layout_changed,
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
            if pane.session.handle().take_screen_dirty() {
                screen_dirty_panes.push(pane.id);
            }
        }
        screen_dirty_panes
    }
}

/// Apply live runtime terminal titles to layout metadata through the shared runtime lock.
pub fn sync_layout_terminal_titles(
    layout: &mut SessionLayout,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<()> {
    let runtimes = crate::server::lock_mutex(runtimes, "pane runtimes")?;
    let _ = runtimes.sync_layout_terminal_titles(layout)?;
    drop(runtimes);
    Ok(())
}

pub fn spawn_pane_or_restore_layout(
    layout: &mut SessionLayout,
    previous_layout: SessionLayout,
    pane_id: PaneId,
    config: &ServerConfig,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneId> {
    // New panes update layout and runtimes together; rollback the layout if PTY spawn fails so render cannot see
    // pane metadata without a runtime.
    let cwd = layout
        .pane(pane_id)
        .map(|pane| pane.cwd.clone())
        .ok_or_else(|| report!("muxr new pane is missing from server layout").attach(format!("pane_id={pane_id}")))?;
    let spawn_result = match crate::server::lock_mutex(runtimes, "pane runtimes") {
        Ok(mut runtimes) => runtimes.spawn_pane(pane_id, &cwd, config, terminal_size),
        Err(error) => Err(error),
    };
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
        PaneRuntimes { panes: Vec::new() }
    }
}
