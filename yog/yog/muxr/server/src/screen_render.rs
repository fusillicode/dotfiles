use std::time::Instant;

use muxr_core::AttachAccepted;
use muxr_core::ClientMousePosition;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::RenderUpdate;
use muxr_core::ServerEvent;
use muxr_core::SessionPaths;
use muxr_core::TerminalSize;
use muxr_transport::ServerEventWriter;

use crate::client::session::ClientSessionState;
use crate::client::timers::ClientTimers;
use crate::pane::fullscreen::PaneFullscreen;
use crate::pane::layout::PaneLayout;
use crate::pane::layout::PaneRegion;
use crate::pane::render::PaneRenderConfig;
use crate::pane::render::PaneRenderLayout;
use crate::pane::render::RenderComposer;
use crate::pane::render::RenderDiffReason;
use crate::pane::runtime::PaneRuntimeMetadata;
use crate::pane::runtime::PaneRuntimes;
use crate::pane::tracked_process::PaneTrackedProcessSnapshot;
use crate::pane::tracked_process::PaneTrackedProcesses;
use crate::pane::tracked_process::TrackedProcessAttention;
use crate::server::ServerConfig;
use crate::state::SessionLayout;

pub fn resize_panes_to_layout(
    layout: &SessionLayout,
    runtimes: &PaneRuntimes,
    size: &TerminalSize,
) -> rootcause::Result<()> {
    let regions = layout.pane_regions(size)?;
    runtimes.resize_panes(&regions)
}

pub fn initial_client_render(
    config: &ServerConfig,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    pane_tracked_processes: &PaneTrackedProcesses,
    terminal_size: &TerminalSize,
) -> rootcause::Result<(LayoutSnapshot, PaneRegionsSnapshot, RenderComposer, RenderUpdate)> {
    let mut render_composer = RenderComposer::default();
    let tracked_processes = pane_tracked_processes.snapshot();
    let layout_snapshot = self::layout_snapshot_and_persist(&config.paths, layout, runtimes, &tracked_processes)?;
    let pane_layout = PaneFullscreen::default().pane_layout(layout, terminal_size)?;
    let pane_regions = self::pane_regions_snapshot(&pane_layout, runtimes)?;
    let attention_panes = self::attention_pane_ids(layout, pane_tracked_processes);
    let render_baseline = render_composer.render_baseline(
        PaneRenderConfig {
            border_styles: config.user_config.pane_borders,
            mode: crate::pane::borders::BorderRenderMode::Focus,
            pane_attention: config.user_config.pane_attention,
            pane_dim: config.user_config.pane_dim,
        },
        PaneRenderLayout {
            active_pane: layout.active_pane_id()?,
            pane_layout: &pane_layout,
        },
        runtimes,
        terminal_size,
        &attention_panes,
    )?;
    Ok((layout_snapshot, pane_regions, render_composer, render_baseline))
}

fn layout_snapshot_and_persist(
    paths: &SessionPaths,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    tracked_processes: &PaneTrackedProcessSnapshot,
) -> rootcause::Result<LayoutSnapshot> {
    self::layout_snapshot_and_maybe_persist(paths, layout, runtimes, tracked_processes, true)
}

fn layout_snapshot_and_maybe_persist(
    paths: &SessionPaths,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    tracked_processes: &PaneTrackedProcessSnapshot,
    persist_layout: bool,
) -> rootcause::Result<LayoutSnapshot> {
    let synced = runtimes.sync_layout_terminal_titles(layout)?;
    if persist_layout && synced.layout_changed() {
        crate::state::persisted::write_metadata(paths, layout)?;
    }
    let runtime_metadata = PaneRuntimeMetadata::from_sources(
        synced.titles().to_vec(),
        runtimes.startup_cmd_labels(),
        tracked_processes,
    );
    layout.snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())
}

fn pane_regions_snapshot(pane_layout: &PaneLayout, runtimes: &PaneRuntimes) -> rootcause::Result<PaneRegionsSnapshot> {
    let regions = pane_layout
        .regions()
        .iter()
        .map(|region| self::pane_region_snapshot(region, runtimes))
        .collect::<rootcause::Result<Vec<_>>>()?;
    PaneRegionsSnapshot::new(regions)
}

fn pane_region_snapshot(region: &PaneRegion, runtimes: &PaneRuntimes) -> rootcause::Result<PaneRegionSnapshot> {
    let handle = runtimes.handle(region.id)?;
    let mouse_mode = handle.mouse_mode()?;
    let visible_top_row = handle.visible_top_row()?;
    PaneRegionSnapshot::new(
        region.id,
        region.area.origin.col,
        region.area.origin.row,
        region.area.size.cols,
        region.area.size.rows,
        mouse_mode,
        visible_top_row,
    )
}

fn visible_pane_region_at_position(
    state: &ClientSessionState<'_>,
    position: ClientMousePosition,
) -> rootcause::Result<Option<PaneRegion>> {
    Ok(self::visible_pane_layout(state)?
        .regions()
        .iter()
        .find(|region| region.contains(position.into()))
        .cloned())
}

pub fn visible_pane_id_at_position(
    state: &ClientSessionState<'_>,
    position: ClientMousePosition,
) -> rootcause::Result<Option<PaneId>> {
    Ok(self::visible_pane_region_at_position(state, position)?.map(|region| region.id))
}

pub fn visible_pane_region_snapshot_at_position(
    state: &ClientSessionState<'_>,
    position: ClientMousePosition,
) -> rootcause::Result<Option<PaneRegionSnapshot>> {
    let Some(region) = self::visible_pane_region_at_position(state, position)? else {
        return Ok(None);
    };
    Ok(Some(self::pane_region_snapshot(&region, state.runtimes)?))
}

fn attention_pane_ids(layout: &SessionLayout, pane_tracked_processes: &PaneTrackedProcesses) -> Vec<PaneId> {
    let mut pane_ids = layout.attention_pane_ids();
    for pane_id in pane_tracked_processes.attention_pane_ids(layout) {
        if !pane_ids.contains(&pane_id) {
            pane_ids.push(pane_id);
        }
    }
    pane_ids
}

pub async fn send_attach_response_and_baseline(
    event_writer: &mut ServerEventWriter,
    layout: LayoutSnapshot,
    pane_regions: PaneRegionsSnapshot,
    render_baseline: RenderUpdate,
    client_write_timeout: std::time::Duration,
) -> rootcause::Result<bool> {
    if !crate::event_writer::send_event_with_timeout(
        event_writer,
        &ServerEvent::Attached(AttachAccepted { layout, pane_regions }),
        client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    crate::event_writer::send_event_with_timeout(
        event_writer,
        &ServerEvent::Render(render_baseline),
        client_write_timeout,
    )
    .await
}

pub fn pane_ids_include_visible(
    layout: &SessionLayout,
    pane_fullscreen: &PaneFullscreen,
    terminal_size: &TerminalSize,
    pane_ids: &[PaneId],
) -> rootcause::Result<bool> {
    if pane_ids.is_empty() {
        return Ok(false);
    }
    Ok(pane_fullscreen
        .pane_layout(layout, terminal_size)?
        .regions()
        .iter()
        .any(|region| pane_ids.contains(&region.id)))
}

pub async fn flush_render_diff(
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    if !*render_dirty {
        return Ok(true);
    }

    let (pane_regions, render_update) = {
        let pane_layout = self::visible_pane_layout(state)?;
        let pane_regions = self::pane_regions_snapshot(&pane_layout, state.runtimes)?;
        let attention_panes = self::attention_pane_ids(state.layout, &state.pane_tracked_processes);
        let reason = if pane_regions == state.pane_regions {
            RenderDiffReason::DirtyFrame
        } else {
            // Scrollback can move the viewport without changing the visible pixels. Send an empty diff in that case so
            // clients can complete scroll-dependent state after the matching PaneRegions event.
            RenderDiffReason::RegionChanged
        };
        let update = state.render_composer.render_diff(
            PaneRenderConfig {
                border_styles: state.config.user_config.pane_borders,
                mode: crate::keyboard_input::border_render_mode(state.input_mode),
                pane_attention: state.config.user_config.pane_attention,
                pane_dim: state.config.user_config.pane_dim,
            },
            PaneRenderLayout {
                active_pane: state.layout.active_pane_id()?,
                pane_layout: &pane_layout,
            },
            state.runtimes,
            &state.terminal_size,
            &attention_panes,
            reason,
        )?;
        (pane_regions, update)
    };
    if !self::send_pane_regions_and_render(event_writer, state, pane_regions, render_update).await? {
        return Ok(false);
    }
    *render_dirty = false;
    Ok(true)
}

fn runtime_pane_metadata(state: &ClientSessionState<'_>) -> rootcause::Result<PaneRuntimeMetadata> {
    let terminal_titles = state.runtimes.terminal_titles()?;
    let startup_cmd_labels = state.runtimes.startup_cmd_labels();
    let tracked_processes = state.pane_tracked_processes.snapshot();
    Ok(PaneRuntimeMetadata::from_sources(
        terminal_titles,
        startup_cmd_labels,
        &tracked_processes,
    ))
}

pub async fn flush_cmd_label_layout(
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    title_changes: Vec<(PaneId, Option<String>)>,
) -> rootcause::Result<bool> {
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let changes = {
        let mut last_layout_snapshot = state.last_layout_snapshot.clone();
        let mut layout_changed = false;
        let mut changes = Vec::new();
        for (pane_id, title) in title_changes {
            layout_changed |= state.layout.sync_terminal_titles(&[(pane_id, title.clone())]);
            let runtime_metadata = runtime_metadata.with_terminal_title_override(pane_id, title);
            let layout_snapshot = state
                .layout
                .snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?;
            if layout_snapshot == last_layout_snapshot {
                continue;
            }
            last_layout_snapshot = layout_snapshot.clone();
            changes.push(layout_snapshot);
        }
        if layout_changed && state.scrollback_editor.is_none() {
            crate::state::persisted::write_metadata(&state.config.paths, state.layout)?;
        }
        changes
    };

    for layout_snapshot in changes {
        // Terminal-title changes affect only sidebar metadata; avoid rebuilding the pane frame for cmd/cwd churn.
        if !self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub async fn handle_cmd_handoff_sample(
    timers: &mut ClientTimers,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let pane_ids = timers.take_cmd_handoff_sample_panes()?;
    if pane_ids.is_empty() {
        return Ok(true);
    }

    let changed = state.pane_tracked_processes.observe_runtime_pane_cmds(
        state.config.user_config.as_ref(),
        state.runtimes,
        &pane_ids,
        Instant::now(),
    )?;
    timers.sync_tracked_process_quiet_deadline(state.pane_tracked_processes.next_quiet_deadline()?)?;
    if changed {
        let pane_surface_dirty =
            self::pane_ids_include_visible(state.layout, &state.pane_fullscreen, &state.terminal_size, &pane_ids)?;
        return self::flush_tracked_process_runtime_layout(
            timers,
            event_writer,
            state,
            render_dirty,
            pane_surface_dirty,
        )
        .await;
    }
    Ok(true)
}

pub async fn flush_tracked_process_runtime_layout(
    timers: &mut ClientTimers,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    render_dirty: &mut bool,
    pane_surface_dirty: bool,
) -> rootcause::Result<bool> {
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let layout_snapshot = state
        .layout
        .snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?;
    *render_dirty |= pane_surface_dirty;
    timers.sync_render_deadline(*render_dirty)?;
    self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await
}

pub async fn flush_pane_attention(
    timers: &mut ClientTimers,
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let now = Instant::now();
    let pane_surface_dirty = match state.pane_tracked_processes.mark_quiet_deadlines(state.layout, now)? {
        TrackedProcessAttention::Seen => false,
        TrackedProcessAttention::Unseen { pane_ids } => {
            self::pane_ids_include_visible(state.layout, &state.pane_fullscreen, &state.terminal_size, &pane_ids)?
        }
        TrackedProcessAttention::Unchanged => return Ok(true),
    };
    *render_dirty |= pane_surface_dirty;
    timers.sync_render_deadline(*render_dirty)?;
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let layout_snapshot = state
        .layout
        .snapshot_with_runtime_metadata(&runtime_metadata.pane_snapshot_fields())?;

    self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await
}

async fn send_sidebar_layout_if_changed(
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    layout_snapshot: LayoutSnapshot,
) -> rootcause::Result<bool> {
    if layout_snapshot == state.last_layout_snapshot {
        return Ok(true);
    }
    if !crate::event_writer::send_event_with_timeout(
        event_writer,
        &ServerEvent::SidebarLayout(layout_snapshot.clone()),
        state.config.client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    state.last_layout_snapshot = layout_snapshot;
    Ok(true)
}

async fn send_pane_regions_and_render(
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
    pane_regions: PaneRegionsSnapshot,
    render_update: Option<RenderUpdate>,
) -> rootcause::Result<bool> {
    // Region metadata must precede the render using it: selection/copy translate visible cells through
    // `visible_top_row`, so tab-bar-only renders still need the same ordering as normal pane renders.
    if pane_regions != state.pane_regions {
        if !crate::event_writer::send_event_with_timeout(
            event_writer,
            &ServerEvent::PaneRegions(pane_regions.clone()),
            state.config.client_write_timeout,
        )
        .await?
        {
            return Ok(false);
        }
        state.pane_regions = pane_regions;
    }
    if let Some(render_update) = render_update
        && !crate::event_writer::send_event_with_timeout(
            event_writer,
            &ServerEvent::Render(render_update),
            state.config.client_write_timeout,
        )
        .await?
    {
        return Ok(false);
    }
    Ok(true)
}

pub async fn send_layout_and_baseline(
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<bool> {
    let (layout_snapshot, pane_regions, render_update) = {
        let tracked_processes = state.pane_tracked_processes.snapshot();
        let layout_snapshot = self::layout_snapshot_and_maybe_persist(
            &state.config.paths,
            state.layout,
            state.runtimes,
            &tracked_processes,
            state.scrollback_editor.is_none(),
        )?;
        let pane_layout = self::visible_pane_layout(state)?;
        let pane_regions = self::pane_regions_snapshot(&pane_layout, state.runtimes)?;
        let attention_panes = self::attention_pane_ids(state.layout, &state.pane_tracked_processes);
        let render_update = state.render_composer.render_baseline(
            PaneRenderConfig {
                border_styles: state.config.user_config.pane_borders,
                mode: crate::keyboard_input::border_render_mode(state.input_mode),
                pane_attention: state.config.user_config.pane_attention,
                pane_dim: state.config.user_config.pane_dim,
            },
            PaneRenderLayout {
                active_pane: state.layout.active_pane_id()?,
                pane_layout: &pane_layout,
            },
            state.runtimes,
            &state.terminal_size,
            &attention_panes,
        )?;
        (layout_snapshot, pane_regions, render_update)
    };
    if !crate::event_writer::send_event_with_timeout(
        event_writer,
        &ServerEvent::Layout(layout_snapshot.clone()),
        state.config.client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    if !crate::event_writer::send_event_with_timeout(
        event_writer,
        &ServerEvent::PaneRegions(pane_regions.clone()),
        state.config.client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    state.pane_regions = pane_regions;
    if !crate::event_writer::send_event_with_timeout(
        event_writer,
        &ServerEvent::Render(render_update),
        state.config.client_write_timeout,
    )
    .await?
    {
        return Ok(false);
    }
    state.last_layout_snapshot = layout_snapshot;
    Ok(true)
}

pub async fn resize_panes_and_render(
    event_writer: &mut ServerEventWriter,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<bool> {
    let pane_layout = self::visible_pane_layout(state)?;
    state.runtimes.resize_panes(pane_layout.regions())?;
    self::send_layout_and_baseline(event_writer, state).await
}

fn visible_pane_layout(state: &ClientSessionState<'_>) -> rootcause::Result<PaneLayout> {
    state.pane_fullscreen.pane_layout(state.layout, &state.terminal_size)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    use muxr_config::MuxrConfig;
    use muxr_core::SessionName;
    use muxr_core::SessionPaths;
    use rootcause::report;

    use super::*;
    use crate::pane::cmd::PaneCmd;
    use crate::pane::cmd::PaneCmdObservation;
    use crate::pane::runtime::test_helpers as pane_runtime_test_helpers;
    use crate::pane::split::PaneSplitAxis;
    use crate::server::test_helpers as server_test_helpers;
    use crate::session::start_seed::SessionStartSeed;
    use crate::state::SessionMetadata;

    #[test]
    fn test_layout_snapshot_and_persist_when_runtime_cmd_exists_sets_snapshot_cmd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&session, self::metadata("zsh", 1))?;
        let runtimes = pane_runtime_test_helpers::empty_runtimes();
        let pane_id = PaneId::new(1)?;
        let mut tracked_processes = PaneTrackedProcesses::default();
        assert2::assert!(tracked_processes.observe_pane_cmd(
            &MuxrConfig::default(),
            pane_id,
            &PaneCmdObservation::FgCmd {
                cmd: PaneCmd {
                    executable: "codex".to_owned(),
                    path: None,
                    pid: 42,
                },
            },
            Instant::now(),
        ));

        let snapshot =
            self::layout_snapshot_and_persist(&paths, &mut layout, &runtimes, &tracked_processes.snapshot())?;

        let pane = snapshot
            .tabs()
            .first()
            .and_then(|tab| tab.panes().first())
            .ok_or_else(|| report!("expected pane snapshot"))?;
        pretty_assertions::assert_eq!(pane.cmd_label, Some("cx".to_owned()));
        Ok(())
    }

    #[test]
    fn test_initial_client_render_when_detached_output_arrived_does_not_mark_attention() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = server_test_helpers::server_config(tempdir.path(), "work")?;
        config.shell_cmd = server_test_helpers::shell_cmd_with_args("/bin/sh", &["-c", "printf dirty; sleep 30"]);
        let terminal_size = TerminalSize::new(80, 24)?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(
            config.user_config.layout,
            self::metadata("sh", 2),
            PaneSplitAxis::Vertical,
        )?;
        let runtimes = PaneRuntimes::spawn_for_start_seed(
            &config,
            &SessionStartSeed {
                layout: layout.clone(),
                startup_cmds: Vec::new(),
            },
            &terminal_size,
            Arc::new(tokio::sync::Notify::new()),
        )?;
        let inactive_pane = PaneId::new(1)?;
        let active_pane = PaneId::new(2)?;
        self::wait_for_runtime_snapshot_contains(&runtimes, inactive_pane, "dirty")?;
        self::wait_for_runtime_snapshot_contains(&runtimes, active_pane, "dirty")?;

        self::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        drop(self::initial_client_render(
            &config,
            &mut layout,
            &runtimes,
            &PaneTrackedProcesses::default(),
            &terminal_size,
        )?);

        pretty_assertions::assert_eq!(layout.attention_pane_ids(), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_pane_ids_include_visible_when_pane_is_in_inactive_tab_returns_false() -> rootcause::Result<()> {
        let session = SessionName::default();
        let mut layout = SessionLayout::initial(&session, self::metadata("sh", 1))?;
        let inactive_pane = PaneId::new(1)?;
        let active_pane = layout.create_tab(self::metadata("sh", 2))?;
        let fullscreen = PaneFullscreen::default();
        let terminal_size = TerminalSize::new(80, 24)?;

        assert2::assert!(!self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[inactive_pane]
        )?);
        assert2::assert!(self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[active_pane]
        )?);
        assert2::assert!(self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[inactive_pane, active_pane],
        )?);
        assert2::assert!(!self::pane_ids_include_visible(
            &layout,
            &fullscreen,
            &terminal_size,
            &[]
        )?);
        Ok(())
    }

    fn session_paths(base: &Path, raw: &str) -> rootcause::Result<(SessionName, SessionPaths)> {
        let session = raw.parse()?;
        let state_root = base.join("muxr");
        let root = state_root.join("sessions").join(raw);

        Ok((
            session,
            SessionPaths {
                socket: state_root.join("s").join(format!("{raw}.sock")),
                pid: root.join("server.pid"),
                layout: root.join("layout.json"),
                panes: root.join("panes"),
                root,
            },
        ))
    }

    fn wait_for_runtime_snapshot_contains(
        runtimes: &PaneRuntimes,
        pane_id: PaneId,
        needle: &str,
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            let snapshot = runtimes.handle(pane_id)?.render_snapshot()?;
            let rendered = snapshot
                .rows()
                .iter()
                .flat_map(|row| row.cells().iter().map(muxr_core::RenderCell::text))
                .collect::<String>();
            if rendered.contains(needle) {
                return Ok(());
            }
            if started_at.elapsed() > Duration::from_secs(2) {
                return Err(report!("timed out waiting for muxr runtime snapshot").attach(rendered));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }
}
