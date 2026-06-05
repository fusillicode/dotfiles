use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use muxr_core::AttachAccepted;
use muxr_core::AttachRequest;
use muxr_core::ClientKey;
use muxr_core::ClientKeyCode;
use muxr_core::ClientKeyModifiers;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMousePosition;
use muxr_core::ClientRequest;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::PaneScrollDirection;
use muxr_core::RenderBaseline;
use muxr_core::RenderCell;
use muxr_core::RenderCursor;
use muxr_core::RenderDiff;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderUpdate;
use muxr_core::ServerError;
use muxr_core::ServerEvent;
use muxr_core::SessionName;
use muxr_core::SessionPaths;
use muxr_core::TabId;
use muxr_core::TerminalSize;
use muxr_transport::ServerConnection;
use muxr_transport::ServerEventWriter;
use muxr_transport::ServerListener;
use muxr_transport::ServerRequestReader;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::history::pane_output_path;
use crate::pane_agent::PaneAgentDetection;
use crate::pane_agent::PaneAgentDetectionWorker;
use crate::pane_agent::PaneAgents;
use crate::pane_agent::PaneUserInteraction;
use crate::pane_borders::BorderRenderMode;
use crate::pane_close::ClosePaneOutcome;
use crate::pane_close::PaneExitOutcome;
use crate::pane_focus::PaneFocusDirection;
use crate::pane_layout::PaneRegion;
use crate::pane_resize::PaneResizeDirection;
use crate::pane_scroll::PaneScrollAmount;
use crate::pane_split::PaneSplitAxis;
use crate::pty::PtyEvent;
use crate::pty::PtyExitStatus;
use crate::pty::PtyHandle;
use crate::pty::PtySession;
use crate::pty::PtySinkGuard;
use crate::pty::ShellCmd;
use crate::sessions_delete::DeleteSessions;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;
use crate::terminal::TerminalSnapshot;

const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const ATTENTION_POLL_INTERVAL: Duration = Duration::from_millis(250);
const CLIENT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(test)]
const CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
#[cfg(test)]
const CLIENT_HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(500);
#[cfg(not(test))]
const CLIENT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);
const CLIENT_EVENT_POLL_INTERVAL: Duration = Duration::from_millis(10);
#[cfg(test)]
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_millis(500);
#[cfg(not(test))]
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const GROUP_OR_OTHER_PERMISSIONS_MASK: u32 = 0o077;
const OUTPUT_EVENT_CHANNEL_LIMIT: usize = 1024;
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_SOCKET_MODE: u32 = 0o600;
const RENDER_FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    pub session: SessionName,
    pub paths: SessionPaths,
    max_accepted_connections: Option<usize>,
    pub shell_cmd: ShellCmd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClientCmd {
    ClosePane,
    EnterResizeMode,
    ExitMode,
    FocusPane(PaneFocusDirection),
    ResizePane(PaneResizeDirection),
    SplitPane(PaneSplitAxis),
    Tab(TabCmd),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TabCmd {
    Create,
    FocusNext,
    FocusPrevious,
    MoveNext,
    MovePrevious,
}

struct PaneRuntime {
    id: PaneId,
    session: PtySession,
}

pub struct PaneRuntimes {
    panes: Vec<PaneRuntime>,
}

impl PaneRuntimes {
    fn spawn_for_layout(config: &ServerConfig, layout: &SessionLayout, size: &TerminalSize) -> rootcause::Result<Self> {
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

    pub fn remove(&mut self, pane_id: PaneId) {
        self.panes.retain(|pane| pane.id != pane_id);
    }

    const fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    fn exited_panes(&self) -> rootcause::Result<Vec<(PaneId, PtyExitStatus)>> {
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

    fn resize_panes(&self, regions: &[PaneRegion]) -> rootcause::Result<()> {
        for region in regions {
            self.handle(region.id)?
                .resize(&TerminalSize::new(region.area.size.cols, region.area.size.rows)?)?;
        }
        Ok(())
    }

    fn snapshot(&self, pane_id: PaneId) -> rootcause::Result<TerminalSnapshot> {
        self.handle(pane_id)?.render_snapshot()
    }

    fn terminal_titles(&self) -> rootcause::Result<Vec<(PaneId, Option<String>)>> {
        self.panes
            .iter()
            .filter_map(|pane| match pane.session.handle().terminal_title() {
                Ok(Some(title)) => Some(Ok((pane.id, Some(title)))),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    fn shell_processes(&self) -> rootcause::Result<Vec<(PaneId, Option<u32>)>> {
        self.panes
            .iter()
            .map(|pane| Ok((pane.id, pane.session.handle().process_id()?)))
            .collect()
    }

    fn take_title_changes(&self) -> rootcause::Result<Vec<(PaneId, Option<String>)>> {
        let mut title_changes = Vec::new();
        for pane in &self.panes {
            for title in pane.session.handle().take_title_changes()? {
                title_changes.push((pane.id, title));
            }
        }
        Ok(title_changes)
    }

    fn take_screen_dirty_panes(&self) -> Vec<PaneId> {
        let mut screen_dirty_panes = Vec::new();
        for pane in &self.panes {
            if pane.session.handle().take_screen_dirty() {
                screen_dirty_panes.push(pane.id);
            }
        }
        screen_dirty_panes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompositeFrame {
    cursor: RenderCursor,
    rows: Vec<RenderRowSpan>,
    seq: u64,
    size: TerminalSize,
}

struct RenderComposer {
    last_sent: Option<CompositeFrame>,
    next_seq: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderDiffReason {
    DirtyFrame,
    RegionChanged,
}

impl Default for RenderComposer {
    fn default() -> Self {
        Self {
            last_sent: None,
            next_seq: 1,
        }
    }
}

impl RenderComposer {
    fn render_baseline(
        &mut self,
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        border_mode: BorderRenderMode,
    ) -> rootcause::Result<RenderUpdate> {
        self.render_frame_baseline(Self::current_frame(
            layout,
            runtimes,
            size,
            attention_panes,
            border_mode,
        )?)
    }

    fn render_frame_baseline(&mut self, mut frame: CompositeFrame) -> rootcause::Result<RenderUpdate> {
        frame.seq = self.next_sequence()?;
        let baseline = RenderBaseline::new(frame.seq, frame.size.clone(), frame.cursor.clone(), frame.rows.clone())?;
        self.last_sent = Some(frame);
        Ok(RenderUpdate::Baseline(baseline))
    }

    fn render_diff(
        &mut self,
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        reason: RenderDiffReason,
        border_mode: BorderRenderMode,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let Some(previous) = self.last_sent.as_ref() else {
            return Ok(Some(self.render_baseline(
                layout,
                runtimes,
                size,
                attention_panes,
                border_mode,
            )?));
        };
        let frame = Self::current_frame(layout, runtimes, size, attention_panes, border_mode)?;
        if frame.size != previous.size {
            return Ok(Some(self.render_frame_baseline(frame)?));
        }

        self.render_frame_diff(frame, reason)
    }

    fn render_frame_diff(
        &mut self,
        mut frame: CompositeFrame,
        reason: RenderDiffReason,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let Some(previous) = self.last_sent.as_ref() else {
            return Ok(Some(self.render_frame_baseline(frame)?));
        };
        let (previous_seq, cursor_changed, rows) = {
            let rows = previous
                .rows
                .iter()
                .zip(frame.rows.iter())
                .filter(|(previous_row, current_row)| previous_row != current_row)
                .map(|(_previous_row, current_row)| current_row.clone())
                .collect::<Vec<_>>();
            (previous.seq, frame.cursor != previous.cursor, rows)
        };
        if rows.is_empty() && !cursor_changed && reason == RenderDiffReason::DirtyFrame {
            return Ok(None);
        }

        frame.seq = self.next_sequence()?;
        let diff = RenderDiff::new(previous_seq, frame.seq, frame.size.clone(), frame.cursor.clone(), rows)?;
        self.last_sent = Some(frame);
        Ok(Some(RenderUpdate::Diff(diff)))
    }

    fn current_frame(
        layout: &SessionLayout,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        border_mode: BorderRenderMode,
    ) -> rootcause::Result<CompositeFrame> {
        let pane_layout = layout.pane_layout(size)?;
        let active_pane = layout.active_pane_id()?;
        let mut rows = self::empty_render_rows(size);
        let mut cursor = RenderCursor {
            row: 0,
            col: 0,
            visible: false,
        };

        for region in pane_layout.regions() {
            let snapshot = runtimes.snapshot(region.id)?;
            self::paste_snapshot(&mut rows, region, &snapshot)?;
            if region.id == active_pane && snapshot.cursor().visible {
                let row = region
                    .area
                    .origin
                    .row
                    .checked_add(snapshot.cursor().row)
                    .ok_or_else(|| report!("muxr composite cursor row overflowed"))?;
                let col = region
                    .area
                    .origin
                    .col
                    .checked_add(snapshot.cursor().col)
                    .ok_or_else(|| report!("muxr composite cursor col overflowed"))?;
                cursor = RenderCursor {
                    row,
                    col,
                    visible: true,
                };
            }
        }
        crate::pane_borders::paste_borders(
            &mut rows,
            pane_layout.borders(),
            Some(&active_pane),
            attention_panes,
            border_mode,
        )?;

        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(row, cells)| {
                let row = u16::try_from(row).context("muxr composite render row overflowed")?;
                RenderRowSpan::new(row, 0, cells)
            })
            .collect::<rootcause::Result<Vec<_>>>()?;

        Ok(CompositeFrame {
            cursor,
            rows,
            seq: 0,
            size: size.clone(),
        })
    }

    fn next_sequence(&mut self) -> rootcause::Result<u64> {
        let seq = self.next_seq;
        self.next_seq = self
            .next_seq
            .checked_add(1)
            .ok_or_else(|| report!("muxr composite render sequence overflowed"))?;
        Ok(seq)
    }
}

struct ServerFilesGuard {
    paths: SessionPaths,
}

impl Drop for ServerFilesGuard {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.paths.socket));
        drop(fs::remove_file(&self.paths.pid));
    }
}

struct ClientSlotGuard<'a> {
    active_client: &'a AtomicBool,
}

impl Drop for ClientSlotGuard<'_> {
    fn drop(&mut self) {
        self.active_client.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ServerInputMode {
    #[default]
    Normal,
    Resize,
}

const fn border_render_mode(input_mode: ServerInputMode) -> BorderRenderMode {
    match input_mode {
        ServerInputMode::Normal => BorderRenderMode::Focus,
        ServerInputMode::Resize => BorderRenderMode::Resize,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum KeyResolution {
    Cmd(ClientCmd),
    Raw,
}

/// Run the muxr server for one internally launched session.
///
/// # Errors
/// - Server startup, socket IO, PTY setup, or pid file persistence fails.
pub fn serve_session(session: &SessionName) -> rootcause::Result<()> {
    let paths = SessionPaths::from_home(session)?;

    self::serve(&ServerConfig {
        session: session.clone(),
        paths,
        max_accepted_connections: None,
        shell_cmd: ShellCmd::default_from_env()?,
    })
}

fn serve(config: &ServerConfig) -> rootcause::Result<()> {
    self::run_async(self::serve_async(config))
}

async fn serve_async(config: &ServerConfig) -> rootcause::Result<()> {
    if matches!(config.max_accepted_connections, Some(0)) {
        return Ok(());
    }

    self::prepare_session_dirs(&config.paths)?;
    let listener = ServerListener::bind(&config.paths.socket)?;
    // Own the socket file as soon as bind succeeds so later startup failures do not leave stale sockets.
    let _files_guard = ServerFilesGuard {
        paths: config.paths.clone(),
    };
    self::secure_socket_file(&config.paths.socket)?;
    fs::write(&config.paths.pid, std::process::id().to_string()).context("failed to write muxr server pid")?;
    let initial_size = TerminalSize::new(80, 24)?;
    let metadata = self::session_metadata(config)?;
    let layout = match crate::state::persisted::load_metadata(&config.paths, &config.session)? {
        Some(layout) => layout,
        None => SessionLayout::initial(&config.session, metadata)?,
    };
    let runtimes = PaneRuntimes::spawn_for_layout(config, &layout, &initial_size)?;
    let layout = Arc::new(Mutex::new(layout));
    let runtimes = Arc::new(Mutex::new(runtimes));
    {
        let locked_layout = self::lock_mutex(layout.as_ref(), "layout")?;
        crate::state::persisted::write_metadata(&config.paths, &locked_layout)?;
    }
    let active_client = Arc::new(AtomicBool::new(false));
    let delete_sessions = Arc::new(DeleteSessions::default());
    let mut accepted_connections = 0_usize;
    let mut handles = Vec::new();

    loop {
        if delete_sessions.is_requested() {
            break;
        }

        if matches!(
            self::reap_exited_panes(&config.paths, &layout, &runtimes)?,
            ReapResult::Final
        ) || self::lock_mutex(runtimes.as_ref(), "pane runtimes")?.is_empty()
        {
            break;
        }

        self::join_finished_client_tasks(&mut handles).await?;

        tokio::select! {
            accepted = listener.accept() => {
                let connection = accepted?;
                accepted_connections = accepted_connections
                    .checked_add(1)
                    .ok_or_else(|| report!("muxr accepted connection count overflowed"))?;
                self::spawn_client_task(
                    config,
                    &active_client,
                    &delete_sessions,
                    &layout,
                    &runtimes,
                    connection,
                    &mut handles,
                );

                if let Some(max_accepted_connections) = config.max_accepted_connections
                    && accepted_connections >= max_accepted_connections
                {
                    break;
                }
            }
            () = tokio::time::sleep(ACCEPT_POLL_INTERVAL) => {}
        }
    }

    self::join_client_tasks(handles).await?;
    if delete_sessions.is_requested() {
        drop(runtimes);
        drop(layout);
        crate::sessions_delete::remove_session_files(&config.paths)?;
    }
    Ok(())
}

fn prepare_session_dirs(paths: &SessionPaths) -> rootcause::Result<()> {
    let sessions_root = paths
        .root
        .parent()
        .ok_or_else(|| report!("muxr session root has no parent"))?;
    let socket_root = paths
        .socket
        .parent()
        .ok_or_else(|| report!("muxr socket path has no parent"))?;
    let state_root = socket_root
        .parent()
        .ok_or_else(|| report!("muxr socket root has no parent"))?;

    // Socket names are deterministic, so every muxr-owned directory that can expose them must be private.
    for (path, label) in [
        (state_root, "state root"),
        (sessions_root, "sessions root"),
        (socket_root, "socket root"),
        (paths.root.as_path(), "session root"),
        (paths.panes.as_path(), "panes root"),
    ] {
        self::ensure_private_dir(path, label)?;
    }

    Ok(())
}

fn ensure_private_dir(path: &Path, label: &str) -> rootcause::Result<()> {
    fs::create_dir_all(path).context(format!("failed to create muxr {label}"))?;
    let metadata = fs::symlink_metadata(path).context(format!("failed to inspect muxr {label}"))?;
    if metadata.file_type().is_symlink() {
        return Err(report!("unsafe muxr directory")
            .attach(format!("label={label}"))
            .attach("reason=symlinks are not allowed")
            .attach(format!("path={}", path.display())));
    }
    if !metadata.is_dir() {
        return Err(report!("unsafe muxr directory")
            .attach(format!("label={label}"))
            .attach("reason=path is not a directory")
            .attach(format!("path={}", path.display())));
    }

    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIR_MODE))
        .context(format!("failed to secure muxr {label} permissions"))?;
    self::validate_private_mode(path, label, PRIVATE_DIR_MODE)
}

fn secure_socket_file(path: &Path) -> rootcause::Result<()> {
    // The directory is private, but the socket itself should not be group/other accessible if copied or moved.
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_SOCKET_MODE))
        .context("failed to secure muxr socket file permissions")?;
    self::validate_private_mode(path, "socket file", PRIVATE_SOCKET_MODE)
}

fn validate_private_mode(path: &Path, label: &str, expected_mode: u32) -> rootcause::Result<()> {
    let mode = fs::metadata(path)
        .context(format!("failed to read muxr {label} permissions"))?
        .permissions()
        .mode()
        & 0o777;

    if mode & GROUP_OR_OTHER_PERMISSIONS_MASK != 0 {
        return Err(report!("unsafe muxr permissions")
            .attach(format!("label={label}"))
            .attach(format!("expected={expected_mode:o}"))
            .attach(format!("actual={mode:o}"))
            .attach(format!("path={}", path.display())));
    }

    Ok(())
}

pub fn unix_timestamp_millis() -> rootcause::Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("failed to read system time for muxr layout metadata")?
        .as_millis();

    Ok(u64::try_from(millis).context("muxr layout metadata timestamp overflowed")?)
}

pub fn session_metadata(config: &ServerConfig) -> rootcause::Result<SessionMetadata> {
    Ok(SessionMetadata {
        cmd_label: config.shell_cmd.label(),
        cwd: std::env::current_dir()
            .context("failed to read muxr server cwd")?
            .to_string_lossy()
            .into_owned(),
        started_at: self::unix_timestamp_millis()?,
    })
}

/// Build metadata for panes spawned from the currently active pane.
///
/// New panes inherit the active pane cwd because the server process cwd does not follow interactive `cd` cmds.
pub fn active_pane_session_metadata(
    config: &ServerConfig,
    layout: &SessionLayout,
) -> rootcause::Result<SessionMetadata> {
    let active_pane_id = layout.active_pane_id()?;
    let cwd = layout
        .pane(active_pane_id)
        .map(|pane| pane.cwd.clone())
        .ok_or_else(|| {
            report!("muxr active pane is missing from server layout").attach(format!("pane_id={active_pane_id}"))
        })?;

    Ok(SessionMetadata {
        cmd_label: config.shell_cmd.label(),
        cwd,
        started_at: self::unix_timestamp_millis()?,
    })
}

/// Apply runtime terminal titles to layout cwd metadata before a layout mutation.
pub fn sync_layout_terminal_titles(
    layout: &mut SessionLayout,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<()> {
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    let _ = self::sync_layout_terminal_titles_from_runtimes(layout, &runtimes)?;
    Ok(())
}

pub fn lock_mutex<'a, T>(mutex: &'a Mutex<T>, name: &str) -> rootcause::Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| report!("poisoned muxr {name} mutex"))
}

fn empty_render_rows(size: &TerminalSize) -> Vec<Vec<RenderCell>> {
    let blank = RenderCell::narrow(" ", RenderStyle::default());
    (0..size.rows())
        .map(|_| vec![blank.clone(); usize::from(size.cols())])
        .collect()
}

fn paste_snapshot(
    rows: &mut [Vec<RenderCell>],
    region: &PaneRegion,
    snapshot: &TerminalSnapshot,
) -> rootcause::Result<()> {
    if snapshot.size().cols() != region.area.size.cols || snapshot.size().rows() != region.area.size.rows {
        return Err(report!("muxr pane snapshot size does not match region")
            .attach(format!("pane_id={}", region.id))
            .attach(format!("snapshot_cols={}", snapshot.size().cols()))
            .attach(format!("snapshot_rows={}", snapshot.size().rows()))
            .attach(format!("region_cols={}", region.area.size.cols))
            .attach(format!("region_rows={}", region.area.size.rows)));
    }

    let mut url_links = crate::pane_url_links::detect_visible_url_links(snapshot.rows())?
        .into_iter()
        .peekable();
    for (span_index, span) in snapshot.rows().iter().enumerate() {
        let row = region
            .area
            .origin
            .row
            .checked_add(span.row())
            .ok_or_else(|| report!("muxr pane row offset overflowed"))?;
        let col = region
            .area
            .origin
            .col
            .checked_add(span.col())
            .ok_or_else(|| report!("muxr pane col offset overflowed"))?;
        let target_row = rows
            .get_mut(usize::from(row))
            .ok_or_else(|| report!("muxr pane row outside composite frame"))?;
        let col = usize::from(col);
        let end_col = col
            .checked_add(span.cells().len())
            .ok_or_else(|| report!("muxr pane span end overflowed"))?;
        if end_col > target_row.len() {
            return Err(report!("muxr pane span outside composite frame").attach(format!("pane_id={}", region.id)));
        }
        for (cell_index, (target, cell)) in target_row.iter_mut().skip(col).zip(span.cells().iter()).enumerate() {
            let mut cell = cell.clone();
            if url_links
                .peek()
                .is_some_and(|link| link.row() == span_index && link.cell() == cell_index)
            {
                let link = url_links
                    .next()
                    .ok_or_else(|| report!("muxr pane url link disappeared while pasting snapshot"))?;
                cell = cell.with_hyperlink(link.into_hyperlink());
            }
            *target = cell;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReapResult {
    Final,
    NoExitedPanes,
    Removed,
}

struct AttachedPtySink {
    guard: PtySinkGuard,
    pane_id: PaneId,
}

struct AttachedSessionState<'a> {
    detected_agents: Vec<PaneAgentDetection>,
    detected_agents_refreshed_at: Option<Instant>,
    agent_detection_worker: PaneAgentDetectionWorker,
    pane_agents: PaneAgents,
    config: &'a ServerConfig,
    delete_sessions: &'a DeleteSessions,
    input_mode: ServerInputMode,
    last_layout_snapshot: LayoutSnapshot,
    layout: &'a Mutex<SessionLayout>,
    pane_regions: PaneRegionsSnapshot,
    pending_visible_activity_panes: BTreeSet<PaneId>,
    pty_event_sender: &'a mpsc::SyncSender<PtyEvent>,
    render_composer: &'a mut RenderComposer,
    runtimes: &'a Mutex<PaneRuntimes>,
    sink_guards: &'a mut Vec<AttachedPtySink>,
    terminal_size: TerminalSize,
}

struct AttachedClientTimers {
    attention_tick: tokio::time::Interval,
    heartbeat: tokio::time::Interval,
    render_tick: tokio::time::Interval,
    shell_poll: tokio::time::Interval,
}

impl AttachedClientTimers {
    fn new() -> rootcause::Result<Self> {
        let heartbeat_start = tokio::time::Instant::now()
            .checked_add(CLIENT_HEARTBEAT_INTERVAL)
            .ok_or_else(|| report!("muxr heartbeat interval overflowed"))?;
        let render_start = tokio::time::Instant::now()
            .checked_add(RENDER_FRAME_INTERVAL)
            .ok_or_else(|| report!("muxr render frame interval overflowed"))?;
        let attention_start = tokio::time::Instant::now()
            .checked_add(ATTENTION_POLL_INTERVAL)
            .ok_or_else(|| report!("muxr attention poll interval overflowed"))?;

        Ok(Self {
            attention_tick: tokio::time::interval_at(attention_start, ATTENTION_POLL_INTERVAL),
            heartbeat: tokio::time::interval_at(heartbeat_start, CLIENT_HEARTBEAT_INTERVAL),
            render_tick: tokio::time::interval_at(render_start, RENDER_FRAME_INTERVAL),
            shell_poll: tokio::time::interval(CLIENT_EVENT_POLL_INTERVAL),
        })
    }
}

fn attach_pane_sinks(
    runtimes: &Mutex<PaneRuntimes>,
    sender: &mpsc::SyncSender<PtyEvent>,
) -> rootcause::Result<Vec<AttachedPtySink>> {
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    runtimes
        .panes
        .iter()
        .map(|pane| {
            Ok(AttachedPtySink {
                guard: pane.session.handle().attach_sink(sender.clone())?,
                pane_id: pane.id,
            })
        })
        .collect()
}

fn attach_pane_sink(
    runtimes: &Mutex<PaneRuntimes>,
    sender: &mpsc::SyncSender<PtyEvent>,
    pane_id: PaneId,
) -> rootcause::Result<AttachedPtySink> {
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    Ok(AttachedPtySink {
        guard: runtimes.handle(pane_id)?.attach_sink(sender.clone())?,
        pane_id,
    })
}

fn resize_panes_to_layout(
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    size: &TerminalSize,
) -> rootcause::Result<()> {
    let regions = {
        let layout = self::lock_mutex(layout, "layout")?;
        layout.pane_regions(size)?
    };
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    runtimes.resize_panes(&regions)
}

fn reap_exited_panes(
    paths: &SessionPaths,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<ReapResult> {
    let exited_panes = {
        let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
        runtimes.exited_panes()?
    };
    if exited_panes.is_empty() {
        return Ok(ReapResult::NoExitedPanes);
    }

    let exited_at = self::unix_timestamp_millis()?;
    let mut result = ReapResult::Removed;
    {
        let mut layout = self::lock_mutex(layout, "layout")?;
        {
            let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
            let _ = self::sync_layout_terminal_titles_from_runtimes(&mut layout, &runtimes)?;
        }
        let mut removed_panes = Vec::new();
        for (pane_id, exit_status) in &exited_panes {
            match layout.remove_exited_pane(*pane_id, exited_at, exit_status.clone())? {
                PaneExitOutcome::Final => result = ReapResult::Final,
                PaneExitOutcome::Removed => {}
            }
            removed_panes.push(pane_id);
        }
        crate::state::persisted::write_metadata(paths, &layout)?;
        drop(layout);

        let mut runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
        for pane_id in removed_panes {
            runtimes.remove(*pane_id);
        }
        drop(runtimes);
    }

    Ok(result)
}

fn spawn_client_task(
    config: &ServerConfig,
    active_client: &Arc<AtomicBool>,
    delete_sessions: &Arc<DeleteSessions>,
    layout: &Arc<Mutex<SessionLayout>>,
    runtimes: &Arc<Mutex<PaneRuntimes>>,
    connection: ServerConnection,
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) {
    let active_client = Arc::clone(active_client);
    let delete_sessions = Arc::clone(delete_sessions);
    let config = config.clone();
    let layout = Arc::clone(layout);
    let runtimes = Arc::clone(runtimes);
    handles.push(tokio::spawn(async move {
        self::handle_client(
            &config,
            connection,
            &active_client,
            &delete_sessions,
            &layout,
            &runtimes,
        )
        .await
    }));
}

async fn handle_client(
    config: &ServerConfig,
    mut connection: ServerConnection,
    active_client: &AtomicBool,
    delete_sessions: &DeleteSessions,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<()> {
    let Some(attach_request) = self::handle_client_handshake(&mut connection, delete_sessions).await? else {
        return Ok(());
    };

    if active_client.swap(true, Ordering::AcqRel) {
        let _sent = self::send_connection_event_with_timeout(
            &mut connection,
            &ServerEvent::Error(ServerError::ClientAlreadyAttached),
        )
        .await?;
        return Ok(());
    }
    let _client_slot_guard = ClientSlotGuard { active_client };

    if attach_request.session != config.session {
        let _sent = self::send_connection_event_with_timeout(
            &mut connection,
            &ServerEvent::Error(ServerError::SessionMismatch {
                expected: config.session.clone(),
                actual: attach_request.session.clone(),
            }),
        )
        .await?;
        return Ok(());
    }

    self::resize_panes_to_layout(layout, runtimes, &attach_request.terminal_size)?;
    let (pty_event_sender, pty_event_receiver) = mpsc::sync_channel(OUTPUT_EVENT_CHANNEL_LIMIT);
    let mut sink_guards = self::attach_pane_sinks(runtimes, &pty_event_sender)?;
    let (mut request_reader, mut event_writer) = connection.split();
    let (layout_snapshot, pane_regions, mut render_composer, render_baseline) =
        self::initial_attached_render(&config.paths, layout, runtimes, &attach_request.terminal_size)?;
    let last_layout_snapshot = layout_snapshot.clone();
    let attached_pane_regions = pane_regions.clone();
    if !self::send_attached_response_and_baseline(&mut event_writer, layout_snapshot, pane_regions, render_baseline)
        .await?
    {
        return Ok(());
    }

    let (async_pty_sender, mut async_pty_receiver) = tokio::sync::mpsc::channel(OUTPUT_EVENT_CHANNEL_LIMIT);
    let bridge_handle = tokio::task::spawn_blocking(move || {
        while let Ok(event) = pty_event_receiver.recv() {
            if async_pty_sender.blocking_send(event).is_err() {
                break;
            }
        }
    });
    let mut attached_state = AttachedSessionState {
        detected_agents: Vec::new(),
        detected_agents_refreshed_at: None,
        agent_detection_worker: PaneAgentDetectionWorker::default(),
        pane_agents: PaneAgents::default(),
        config,
        delete_sessions,
        input_mode: ServerInputMode::Normal,
        last_layout_snapshot,
        layout,
        pane_regions: attached_pane_regions,
        pending_visible_activity_panes: BTreeSet::new(),
        pty_event_sender: &pty_event_sender,
        render_composer: &mut render_composer,
        runtimes,
        sink_guards: &mut sink_guards,
        terminal_size: attach_request.terminal_size,
    };
    let result = self::run_attached_client(
        &mut request_reader,
        &mut event_writer,
        &mut attached_state,
        &mut async_pty_receiver,
    )
    .await;

    drop(sink_guards);
    drop(pty_event_sender);
    drop(async_pty_receiver);
    bridge_handle
        .await
        .map_err(|error| report!("muxr server pty bridge task panicked").attach(format!("{error}")))?;
    result
}

async fn handle_client_handshake(
    connection: &mut ServerConnection,
    delete_sessions: &DeleteSessions,
) -> rootcause::Result<Option<AttachRequest>> {
    let Ok(Ok(Some(request))) = tokio::time::timeout(CLIENT_HANDSHAKE_TIMEOUT, connection.recv_request()).await else {
        return Ok(None);
    };

    match request {
        ClientRequest::DeleteSession => {
            crate::sessions_delete::handle_handshake_delete(connection, delete_sessions).await?;
            Ok(None)
        }
        ClientRequest::Ping => {
            let _sent = self::send_connection_event_with_timeout(connection, &ServerEvent::Pong).await?;
            Ok(None)
        }
        ClientRequest::Attach(attach_request) => Ok(Some(attach_request)),
        request @ (ClientRequest::Pong
        | ClientRequest::Detach
        | ClientRequest::RenderResync
        | ClientRequest::Resize(_)
        | ClientRequest::Input(_)
        | ClientRequest::Paste(_)
        | ClientRequest::Key(_)
        | ClientRequest::Mouse(_)
        | ClientRequest::ScrollPaneLineAt { .. }
        | ClientRequest::FocusPaneAt(_)
        | ClientRequest::FocusTab(_)) => {
            let _sent = self::send_connection_event_with_timeout(
                connection,
                &ServerEvent::Error(ServerError::unexpected_request(request)),
            )
            .await?;
            Ok(None)
        }
    }
}

fn initial_attached_render(
    paths: &SessionPaths,
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
    terminal_size: &TerminalSize,
) -> rootcause::Result<(LayoutSnapshot, PaneRegionsSnapshot, RenderComposer, RenderUpdate)> {
    let mut render_composer = RenderComposer::default();
    let mut layout = self::lock_mutex(layout, "layout")?;
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    let layout_snapshot = self::layout_snapshot_and_persist(paths, &mut layout, &runtimes, &[], &[])?;
    let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, terminal_size)?;
    let attention_panes = layout.attention_pane_ids();
    let render_baseline = render_composer.render_baseline(
        &layout,
        &runtimes,
        terminal_size,
        &attention_panes,
        BorderRenderMode::Focus,
    )?;
    drop(runtimes);
    drop(layout);
    Ok((layout_snapshot, pane_regions, render_composer, render_baseline))
}

struct SyncedTerminalTitles {
    layout_changed: bool,
    titles: Vec<(PaneId, Option<String>)>,
}

fn layout_snapshot_and_persist(
    paths: &SessionPaths,
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
    runtime_cmd_labels: &[(PaneId, Option<String>)],
    runtime_agent_states: &[(PaneId, muxr_core::PaneAgentState)],
) -> rootcause::Result<LayoutSnapshot> {
    let synced = self::sync_layout_terminal_titles_from_runtimes(layout, runtimes)?;
    if synced.layout_changed {
        crate::state::persisted::write_metadata(paths, layout)?;
    }
    layout.snapshot_with_runtime_metadata(&synced.titles, runtime_cmd_labels, runtime_agent_states)
}

fn sync_layout_terminal_titles_from_runtimes(
    layout: &mut SessionLayout,
    runtimes: &PaneRuntimes,
) -> rootcause::Result<SyncedTerminalTitles> {
    let terminal_titles = runtimes.terminal_titles()?;
    // Shell prompts often report cwd through OSC title updates; keep layout metadata in sync so the tab bar and
    // subsequently spawned/restored panes use the live cwd instead of the session startup directory.
    let layout_changed = layout.sync_terminal_titles(&terminal_titles);
    Ok(SyncedTerminalTitles {
        layout_changed,
        titles: terminal_titles,
    })
}

fn pane_regions_snapshot(
    layout: &SessionLayout,
    runtimes: &PaneRuntimes,
    terminal_size: &TerminalSize,
) -> rootcause::Result<PaneRegionsSnapshot> {
    let regions = layout
        .pane_regions(terminal_size)?
        .into_iter()
        .map(|region| {
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
        })
        .collect::<rootcause::Result<Vec<_>>>()?;
    PaneRegionsSnapshot::new(regions)
}

fn attention_pane_ids(layout: &SessionLayout, pane_agents: &PaneAgents) -> Vec<PaneId> {
    let mut pane_ids = layout.attention_pane_ids();
    for pane_id in pane_agents.attention_pane_ids(layout) {
        if !pane_ids.contains(&pane_id) {
            pane_ids.push(pane_id);
        }
    }
    pane_ids
}

async fn send_attached_response_and_baseline(
    event_writer: &mut ServerEventWriter,
    layout: LayoutSnapshot,
    pane_regions: PaneRegionsSnapshot,
    render_baseline: RenderUpdate,
) -> rootcause::Result<bool> {
    if !self::send_writer_event_with_timeout(
        event_writer,
        &ServerEvent::Attached(AttachAccepted { layout, pane_regions }),
    )
    .await?
    {
        return Ok(false);
    }
    self::send_writer_event_with_timeout(event_writer, &ServerEvent::Render(render_baseline)).await
}

async fn run_attached_client(
    request_reader: &mut ServerRequestReader,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    pty_event_receiver: &mut tokio::sync::mpsc::Receiver<PtyEvent>,
) -> rootcause::Result<()> {
    let mut timers = AttachedClientTimers::new()?;
    let mut heartbeat_started_at: Option<tokio::time::Instant> = None;
    let mut render_dirty = false;
    let mut request_turn = false;

    loop {
        // A dropped PTY sink means live output is already stale; release the
        // active slot instead of draining old frames into a slow client.
        if !state.sink_guards.iter().all(|sink| sink.guard.is_output_current()) {
            return Ok(());
        }
        if let Some(started_at) = heartbeat_started_at
            && started_at.elapsed() > CLIENT_HEARTBEAT_TIMEOUT
        {
            return Ok(());
        }
        if state.delete_sessions.is_requested() {
            // The delete requester already received the explicit ack; attached clients can observe connection close.
            // Waiting to notify a slow attached terminal would delay server-owned cleanup of the selected session.
            return Ok(());
        }

        if request_turn {
            tokio::select! {
                biased;
                _ = timers.heartbeat.tick() => {
                    if heartbeat_started_at.is_none() {
                        if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Ping).await? {
                            return Ok(());
                        }
                        heartbeat_started_at = Some(tokio::time::Instant::now());
                    }
                },
                _ = timers.shell_poll.tick() => {
                    if self::handle_reaped_panes(state, event_writer).await? {
                        return Ok(());
                    }
                },
                _ = timers.render_tick.tick() => {
                    if !self::flush_render_diff(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                _ = timers.attention_tick.tick() => {
                    if !self::flush_pane_attention(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    if !self::handle_attached_request(request?, event_writer, state, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    request_turn = true;
                    if !self::handle_pty_event(event, event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
            }
        } else {
            tokio::select! {
                biased;
                _ = timers.heartbeat.tick() => {
                    if heartbeat_started_at.is_none() {
                        if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Ping).await? {
                            return Ok(());
                        }
                        heartbeat_started_at = Some(tokio::time::Instant::now());
                    }
                },
                _ = timers.shell_poll.tick() => {
                    if self::handle_reaped_panes(state, event_writer).await? {
                        return Ok(());
                    }
                },
                _ = timers.render_tick.tick() => {
                    if !self::flush_render_diff(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                _ = timers.attention_tick.tick() => {
                    if !self::flush_pane_attention(event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                event = pty_event_receiver.recv() => {
                    // Output gets one turn, then client requests get first chance so detach/pong cannot starve.
                    request_turn = true;
                    if !self::handle_pty_event(event, event_writer, state, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
                request = request_reader.recv_request() => {
                    request_turn = false;
                    if !self::handle_attached_request(request?, event_writer, state, &mut heartbeat_started_at, &mut render_dirty).await? {
                        return Ok(());
                    }
                },
            }
        }
    }
}

async fn handle_pty_event(
    event: Option<PtyEvent>,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match event {
        Some(PtyEvent::Exited) => Ok(!self::handle_reaped_panes(state, event_writer).await?),
        Some(PtyEvent::OutputReady) => {
            let (screen_dirty_panes, title_changes) = {
                let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
                (runtimes.take_screen_dirty_panes(), runtimes.take_title_changes()?)
            };
            let screen_dirty = !screen_dirty_panes.is_empty();
            *render_dirty |= screen_dirty;
            if !title_changes.is_empty() && !self::flush_cmd_label_layout(event_writer, state, title_changes).await? {
                return Ok(false);
            }
            if screen_dirty {
                state.pending_visible_activity_panes.extend(screen_dirty_panes);
            }
            Ok(true)
        }
        None => Ok(false),
    }
}

async fn flush_render_diff(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    if !*render_dirty {
        return Ok(true);
    }

    let (pane_regions, render_update) = {
        let layout = self::lock_mutex(state.layout, "layout")?;
        let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
        let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, &state.terminal_size)?;
        let attention_panes = self::attention_pane_ids(&layout, &state.pane_agents);
        let reason = if pane_regions == state.pane_regions {
            RenderDiffReason::DirtyFrame
        } else {
            // Scrollback can move the viewport without changing the visible pixels. Send an empty diff in that case so
            // clients can complete scroll-dependent state after the matching PaneRegions event.
            RenderDiffReason::RegionChanged
        };
        let update = state.render_composer.render_diff(
            &layout,
            &runtimes,
            &state.terminal_size,
            &attention_panes,
            reason,
            self::border_render_mode(state.input_mode),
        )?;
        drop(runtimes);
        drop(layout);
        (pane_regions, update)
    };
    if !self::send_pane_regions_and_render(event_writer, state, pane_regions, render_update).await? {
        return Ok(false);
    }
    *render_dirty = false;
    Ok(true)
}

struct RuntimePaneMetadata {
    runtime_cmd_labels: Vec<(PaneId, Option<String>)>,
    terminal_titles: Vec<(PaneId, Option<String>)>,
}

fn refresh_detected_agents_if_due(state: &mut AttachedSessionState<'_>, now: Instant) -> rootcause::Result<()> {
    if let Some(detected_agents) = state.agent_detection_worker.take_finished() {
        state.detected_agents = detected_agents;
        state.detected_agents_refreshed_at = Some(now);
    }

    if state.agent_detection_worker.has_pending()
        || !self::detected_agents_refresh_due(state.detected_agents_refreshed_at, now)
    {
        return Ok(());
    }

    // sysinfo refreshes the full process list and can block; the attention tick only submits work and consumes
    // completed scans so agent attention has one timing model.
    let shell_processes = {
        let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
        runtimes.shell_processes()?
    };
    state.agent_detection_worker.request(shell_processes)
}

fn detected_agents_refresh_due(refreshed_at: Option<Instant>, now: Instant) -> bool {
    refreshed_at.is_none_or(|refreshed_at| now.saturating_duration_since(refreshed_at) >= ATTENTION_POLL_INTERVAL)
}

fn runtime_pane_metadata(state: &AttachedSessionState<'_>) -> rootcause::Result<RuntimePaneMetadata> {
    let terminal_titles = {
        let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
        runtimes.terminal_titles()?
    };
    let runtime_cmd_labels = self::runtime_cmd_labels(&state.detected_agents);
    Ok(RuntimePaneMetadata {
        runtime_cmd_labels,
        terminal_titles,
    })
}

fn runtime_cmd_labels(detected_agents: &[PaneAgentDetection]) -> Vec<(PaneId, Option<String>)> {
    detected_agents
        .iter()
        .filter_map(|detection| {
            detection
                .agent()
                .map(|agent| (*detection.pane_id(), Some(agent.short_name().to_owned())))
        })
        .collect()
}

async fn flush_cmd_label_layout(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    title_changes: Vec<(PaneId, Option<String>)>,
) -> rootcause::Result<bool> {
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let changes = {
        let mut layout = self::lock_mutex(state.layout, "layout")?;
        let mut last_layout_snapshot = state.last_layout_snapshot.clone();
        let mut layout_changed = false;
        let runtime_agent_states = state.pane_agents.snapshot_states();
        let mut changes = Vec::new();
        for (pane_id, title) in title_changes {
            layout_changed |= layout.sync_terminal_titles(&[(pane_id, title.clone())]);
            let terminal_titles =
                self::terminal_titles_with_override(&runtime_metadata.terminal_titles, pane_id, title);
            let layout_snapshot = layout.snapshot_with_runtime_metadata(
                &terminal_titles,
                &runtime_metadata.runtime_cmd_labels,
                &runtime_agent_states,
            )?;
            if layout_snapshot == last_layout_snapshot {
                continue;
            }
            last_layout_snapshot = layout_snapshot.clone();
            changes.push(layout_snapshot);
        }
        if layout_changed {
            crate::state::persisted::write_metadata(&state.config.paths, &layout)?;
        }
        drop(layout);
        changes
    };

    for layout_snapshot in changes {
        // Terminal-title changes affect only sidebar metadata; avoid rebuilding the pane frame for command/cwd churn.
        if !self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn flush_pane_attention(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let now = Instant::now();
    self::refresh_detected_agents_if_due(state, now)?;
    let runtime_metadata = self::runtime_pane_metadata(state)?;
    let visible_activity_panes = state.pending_visible_activity_panes.iter().copied().collect::<Vec<_>>();
    state.pending_visible_activity_panes.clear();
    let layout_snapshot = {
        let layout = self::lock_mutex(state.layout, "layout")?;
        if !state
            .pane_agents
            .sync_attention(&layout, &state.detected_agents, &visible_activity_panes, now)?
        {
            return Ok(true);
        }
        *render_dirty = true;
        let runtime_agent_states = state.pane_agents.snapshot_states();
        layout.snapshot_with_runtime_metadata(
            &runtime_metadata.terminal_titles,
            &runtime_metadata.runtime_cmd_labels,
            &runtime_agent_states,
        )?
    };

    self::send_sidebar_layout_if_changed(event_writer, state, layout_snapshot).await
}

async fn send_sidebar_layout_if_changed(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    layout_snapshot: LayoutSnapshot,
) -> rootcause::Result<bool> {
    if layout_snapshot == state.last_layout_snapshot {
        return Ok(true);
    }
    if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::SidebarLayout(layout_snapshot.clone())).await?
    {
        return Ok(false);
    }
    state.last_layout_snapshot = layout_snapshot;
    Ok(true)
}

fn terminal_titles_with_override(
    terminal_titles: &[(PaneId, Option<String>)],
    pane_id: PaneId,
    title: Option<String>,
) -> Vec<(PaneId, Option<String>)> {
    let mut out = Vec::with_capacity(terminal_titles.len().saturating_add(1));
    let mut replaced = false;
    for (existing_pane_id, existing_title) in terminal_titles {
        if *existing_pane_id == pane_id {
            out.push((*existing_pane_id, title.clone()));
            replaced = true;
        } else {
            out.push((*existing_pane_id, existing_title.clone()));
        }
    }
    if !replaced {
        out.push((pane_id, title));
    }
    out
}

async fn send_pane_regions_and_render(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    pane_regions: PaneRegionsSnapshot,
    render_update: Option<RenderUpdate>,
) -> rootcause::Result<bool> {
    // Region metadata must precede the render using it: selection/copy translate visible cells through
    // `visible_top_row`, so tab-bar-only renders still need the same ordering as normal pane renders.
    if pane_regions != state.pane_regions {
        if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::PaneRegions(pane_regions.clone())).await? {
            return Ok(false);
        }
        state.pane_regions = pane_regions;
    }
    if let Some(render_update) = render_update
        && !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Render(render_update)).await?
    {
        return Ok(false);
    }
    Ok(true)
}

async fn send_layout_and_baseline(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let runtime_cmd_labels = self::runtime_cmd_labels(&state.detected_agents);
    let (layout_snapshot, pane_regions, render_update) = {
        let mut layout = self::lock_mutex(state.layout, "layout")?;
        let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
        let runtime_agent_states = state.pane_agents.snapshot_states();
        let layout_snapshot = self::layout_snapshot_and_persist(
            &state.config.paths,
            &mut layout,
            &runtimes,
            &runtime_cmd_labels,
            &runtime_agent_states,
        )?;
        let pane_regions = self::pane_regions_snapshot(&layout, &runtimes, &state.terminal_size)?;
        let attention_panes = self::attention_pane_ids(&layout, &state.pane_agents);
        let render_update = state.render_composer.render_baseline(
            &layout,
            &runtimes,
            &state.terminal_size,
            &attention_panes,
            self::border_render_mode(state.input_mode),
        )?;
        drop(runtimes);
        drop(layout);
        (layout_snapshot, pane_regions, render_update)
    };
    if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Layout(layout_snapshot.clone())).await? {
        return Ok(false);
    }
    if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::PaneRegions(pane_regions.clone())).await? {
        return Ok(false);
    }
    state.pane_regions = pane_regions;
    if !self::send_writer_event_with_timeout(event_writer, &ServerEvent::Render(render_update)).await? {
        return Ok(false);
    }
    state.last_layout_snapshot = layout_snapshot;
    Ok(true)
}

async fn handle_reaped_panes(
    state: &mut AttachedSessionState<'_>,
    event_writer: &mut ServerEventWriter,
) -> rootcause::Result<bool> {
    match self::reap_exited_panes(&state.config.paths, state.layout, state.runtimes)? {
        ReapResult::Final => Ok(true),
        ReapResult::NoExitedPanes => Ok(false),
        ReapResult::Removed => {
            let live_panes = {
                let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
                runtimes.panes.iter().map(|pane| pane.id).collect::<Vec<_>>()
            };
            state.sink_guards.retain(|sink| live_panes.contains(&sink.pane_id));
            Ok(!self::resize_panes_and_render(event_writer, state).await?)
        }
    }
}

async fn resize_panes_and_render(
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    self::resize_panes_to_layout(state.layout, state.runtimes, &state.terminal_size)?;
    self::send_layout_and_baseline(event_writer, state).await
}

fn scroll_pane_line_at_result(
    position: ClientMousePosition,
    direction: PaneScrollDirection,
    state: &AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<ServerEvent> {
    let scrolled = crate::pane_scroll::handle_scroll_pane_at_request(
        position,
        direction,
        PaneScrollAmount::Line,
        state.layout,
        state.runtimes,
        &state.terminal_size,
    )?;
    if scrolled {
        // Edge-drag autoscroll uses one-line steps but can still outpace render IO; keep it coalesced on the normal
        // render tick.
        *render_dirty = true;
    }
    // Clients keep one edge-scroll request pending until either a moved viewport renders or this no-op result arrives.
    Ok(ServerEvent::ScrollPaneLineResult {
        position,
        direction,
        scrolled,
    })
}

async fn handle_attached_request(
    request: Option<ClientRequest>,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    heartbeat_started_at: &mut Option<tokio::time::Instant>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match request {
        Some(ClientRequest::Detach) => {
            let _sent = self::send_writer_event_with_timeout(event_writer, &ServerEvent::Detached).await?;
            Ok(false)
        }
        Some(ClientRequest::DeleteSession) => {
            crate::sessions_delete::handle_attached_delete(event_writer, state.delete_sessions).await?;
            Ok(false)
        }
        Some(ClientRequest::Input(bytes)) => {
            if !bytes.is_empty() {
                *render_dirty |=
                    self::write_active_pane_user_input(state, self::input_interaction(&bytes), |handle| {
                        handle.write_input(&bytes)
                    })?;
            }
            Ok(true)
        }
        Some(ClientRequest::Paste(bytes)) => {
            if !bytes.is_empty() {
                // Bracketed paste can contain newlines as data; only raw input newlines mean prompt submission.
                *render_dirty |= self::write_active_pane_user_input(state, PaneUserInteraction::MayEcho, |handle| {
                    handle.write_paste(&bytes)
                })?;
            }
            Ok(true)
        }
        Some(ClientRequest::Key(key)) => self::handle_key_request(key, event_writer, state, render_dirty).await,
        Some(ClientRequest::Mouse(event)) => {
            self::handle_mouse_event_request(event, event_writer, state, render_dirty).await
        }
        Some(ClientRequest::ScrollPaneLineAt { position, direction }) => {
            let event = self::scroll_pane_line_at_result(position, direction, state, render_dirty)?;
            self::send_writer_event_with_timeout(event_writer, &event).await
        }
        Some(ClientRequest::FocusPaneAt(position)) => {
            if !crate::pane_focus::handle_focus_pane_at_request(
                position,
                state.config,
                state.layout,
                &state.terminal_size,
            )? {
                return Ok(true);
            }
            let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
            if !self::send_layout_and_baseline(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        Some(ClientRequest::FocusTab(tab_id)) => self::handle_focus_tab_request(tab_id, event_writer, state).await,
        Some(ClientRequest::Resize(size)) => {
            state.terminal_size = size;
            if !self::resize_panes_and_render(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        Some(ClientRequest::RenderResync) => {
            if !self::send_layout_and_baseline(event_writer, state).await? {
                return Ok(false);
            }
            Ok(true)
        }
        Some(ClientRequest::Ping) => self::send_writer_event_with_timeout(event_writer, &ServerEvent::Pong).await,
        Some(ClientRequest::Pong) => {
            *heartbeat_started_at = None;
            Ok(true)
        }
        Some(request @ ClientRequest::Attach(_)) => {
            let _sent = self::send_writer_event_with_timeout(
                event_writer,
                &ServerEvent::Error(ServerError::unexpected_request(request)),
            )
            .await?;
            Ok(false)
        }
        None => Ok(false),
    }
}

async fn handle_focus_tab_request(
    tab_id: TabId,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    let changed = {
        let mut layout = self::lock_mutex(state.layout, "layout")?;
        let changed = crate::tab_focus::handle_focus_tab(&mut layout, tab_id)?;
        if changed {
            crate::state::persisted::write_metadata(&state.config.paths, &layout)?;
        }
        drop(layout);
        changed
    };
    if changed {
        let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
        if !self::send_layout_and_baseline(event_writer, state).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn handle_key_request(
    key: ClientKey,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    match self::resolve_key(&mut state.input_mode, &key) {
        KeyResolution::Cmd(cmd) => self::handle_cmd_request(cmd, event_writer, state).await,
        KeyResolution::Raw => {
            if !key.raw_bytes.is_empty() {
                *render_dirty |=
                    self::write_active_pane_user_input(state, self::input_interaction(&key.raw_bytes), |handle| {
                        handle.write_input(&key.raw_bytes)
                    })?;
            }
            Ok(true)
        }
    }
}

async fn handle_cmd_request(
    cmd: ClientCmd,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    match cmd {
        ClientCmd::Tab(cmd) => self::handle_tab_cmd_request(cmd, event_writer, state).await,
        ClientCmd::SplitPane(split_axis) => {
            let pane_id = crate::pane_split::handle_split_pane_cmd(
                split_axis,
                state.config,
                state.layout,
                state.runtimes,
                &state.terminal_size,
            )?;
            state
                .sink_guards
                .push(self::attach_pane_sink(state.runtimes, state.pty_event_sender, pane_id)?);
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCmd::ClosePane => {
            let outcome = crate::pane_close::handle_close_pane_cmd(state.config, state.layout, state.runtimes)?;
            match &outcome {
                ClosePaneOutcome::Final { pane_id } | ClosePaneOutcome::Removed { pane_id } => {
                    state.sink_guards.retain(|sink| &sink.pane_id != pane_id);
                }
            }
            match outcome {
                ClosePaneOutcome::Final { .. } => {
                    let _sent = self::send_writer_event_with_timeout(event_writer, &ServerEvent::Detached).await?;
                    Ok(false)
                }
                ClosePaneOutcome::Removed { .. } => self::resize_panes_and_render(event_writer, state).await,
            }
        }
        ClientCmd::ResizePane(direction) => {
            if !crate::pane_resize::handle_resize_pane_cmd(direction, state.config, state.layout)? {
                return Ok(true);
            }
            self::resize_panes_and_render(event_writer, state).await
        }
        ClientCmd::FocusPane(direction) => {
            if !crate::pane_focus::handle_focus_pane_cmd(direction, state.config, state.layout, &state.terminal_size)? {
                return Ok(true);
            }
            let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
            self::send_layout_and_baseline(event_writer, state).await
        }
        ClientCmd::EnterResizeMode | ClientCmd::ExitMode => self::send_layout_and_baseline(event_writer, state).await,
    }
}

async fn handle_tab_cmd_request(
    cmd: TabCmd,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    match cmd {
        TabCmd::Create => {
            let pane_id = {
                let mut layout = self::lock_mutex(state.layout, "layout")?;
                let pane_id = crate::tab_create::handle_create_tab(
                    &mut layout,
                    state.config,
                    state.runtimes,
                    &state.terminal_size,
                )?;
                crate::state::persisted::write_metadata(&state.config.paths, &layout)?;
                drop(layout);
                pane_id
            };
            state
                .sink_guards
                .push(self::attach_pane_sink(state.runtimes, state.pty_event_sender, pane_id)?);
        }
        TabCmd::FocusPrevious => {
            let mut layout = self::lock_mutex(state.layout, "layout")?;
            crate::tab_focus::handle_focus_previous_tab(&mut layout)?;
            crate::state::persisted::write_metadata(&state.config.paths, &layout)?;
            drop(layout);
            let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
        }
        TabCmd::FocusNext => {
            let mut layout = self::lock_mutex(state.layout, "layout")?;
            crate::tab_focus::handle_focus_next_tab(&mut layout)?;
            crate::state::persisted::write_metadata(&state.config.paths, &layout)?;
            drop(layout);
            let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
        }
        TabCmd::MovePrevious => {
            let mut layout = self::lock_mutex(state.layout, "layout")?;
            crate::tab_move::handle_move_active_tab_previous(&mut layout)?;
            crate::state::persisted::write_metadata(&state.config.paths, &layout)?;
            drop(layout);
        }
        TabCmd::MoveNext => {
            let mut layout = self::lock_mutex(state.layout, "layout")?;
            crate::tab_move::handle_move_active_tab_next(&mut layout)?;
            crate::state::persisted::write_metadata(&state.config.paths, &layout)?;
            drop(layout);
        }
    }
    self::resize_panes_and_render(event_writer, state).await
}

fn active_pane_handle_with_id(
    layout: &Mutex<SessionLayout>,
    runtimes: &Mutex<PaneRuntimes>,
) -> rootcause::Result<(PaneId, PtyHandle)> {
    let active_pane = {
        let layout = self::lock_mutex(layout, "layout")?;
        layout.active_pane_id()?
    };
    let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
    let handle = runtimes.handle(active_pane)?;
    drop(runtimes);
    Ok((active_pane, handle))
}

fn write_active_pane_user_input(
    state: &mut AttachedSessionState<'_>,
    interaction: PaneUserInteraction,
    write: impl FnOnce(&PtyHandle) -> rootcause::Result<bool>,
) -> rootcause::Result<bool> {
    let (pane_id, handle) = self::active_pane_handle_with_id(state.layout, state.runtimes)?;
    let render_dirty = write(&handle)?;
    state
        .pane_agents
        .record_user_interaction(pane_id, interaction, Instant::now());
    Ok(render_dirty)
}

fn acknowledge_active_agent_attention(state: &mut AttachedSessionState<'_>) -> rootcause::Result<bool> {
    let active_pane = {
        let layout = self::lock_mutex(state.layout, "layout")?;
        layout.active_pane_id()?
    };
    Ok(state.pane_agents.acknowledge_attention(active_pane))
}

fn input_interaction(bytes: &[u8]) -> PaneUserInteraction {
    if bytes.contains(&b'\r') || bytes.contains(&b'\n') {
        PaneUserInteraction::StartsAgentWork
    } else {
        PaneUserInteraction::MayEcho
    }
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
    let spawn_result = match self::lock_mutex(runtimes, "pane runtimes") {
        Ok(mut runtimes) => runtimes.spawn_pane(pane_id, &cwd, config, terminal_size),
        Err(error) => Err(error),
    };
    if let Err(error) = spawn_result {
        *layout = previous_layout;
        return Err(error).attach("rolled back muxr layout after pane spawn failure");
    }
    Ok(pane_id)
}

async fn handle_mouse_event_request(
    event: ClientMouseEvent,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
    render_dirty: &mut bool,
) -> rootcause::Result<bool> {
    let Some(region) =
        crate::pane_focus::mouse_event_region(state.layout, state.runtimes, &state.terminal_size, event.position)?
    else {
        return Ok(true);
    };
    let handle = {
        let runtimes = self::lock_mutex(state.runtimes, "pane runtimes")?;
        let handle = runtimes.handle(*region.id())?;
        drop(runtimes);
        handle
    };
    let action = crate::pane_mouse::resolve_pane_mouse_action(event, handle.application_mode()?);
    match action {
        crate::pane_mouse::PaneMouseAction::ForwardToPty { focus, protocol } => {
            if let Some(scrolled_to_bottom) = handle.write_mouse_event(event, &region, protocol)? {
                *render_dirty |= scrolled_to_bottom;
                state
                    .pane_agents
                    .record_user_interaction(*region.id(), PaneUserInteraction::MayEcho, Instant::now());
            }
            if !focus.focuses_pane() {
                return Ok(true);
            }
            self::handle_mouse_focus(event, event_writer, state).await
        }
        crate::pane_mouse::PaneMouseAction::FauxScrollPty {
            application_cursor,
            direction,
        } => {
            *render_dirty |= handle.write_faux_scroll_input(direction, application_cursor)?;
            state
                .pane_agents
                .record_user_interaction(*region.id(), PaneUserInteraction::MayEcho, Instant::now());
            Ok(true)
        }
        crate::pane_mouse::PaneMouseAction::ScrollHistory { direction } => {
            if !crate::pane_scroll::handle_scroll_pane_at_request(
                event.position,
                direction,
                PaneScrollAmount::Wheel,
                state.layout,
                state.runtimes,
                &state.terminal_size,
            )? {
                return Ok(true);
            }
            // Wheel input can arrive much faster than render IO; mark dirty and let the normal render tick coalesce.
            *render_dirty = true;
            Ok(true)
        }
        crate::pane_mouse::PaneMouseAction::FocusPane => self::handle_mouse_focus(event, event_writer, state).await,
        crate::pane_mouse::PaneMouseAction::NoAction => Ok(true),
    }
}

async fn handle_mouse_focus(
    event: ClientMouseEvent,
    event_writer: &mut ServerEventWriter,
    state: &mut AttachedSessionState<'_>,
) -> rootcause::Result<bool> {
    if !crate::pane_focus::handle_focus_pane_at_request(
        event.position,
        state.config,
        state.layout,
        &state.terminal_size,
    )? {
        return Ok(true);
    }
    let _agent_acknowledged = self::acknowledge_active_agent_attention(state)?;
    self::send_layout_and_baseline(event_writer, state).await
}

const fn resolve_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match input_mode {
        ServerInputMode::Normal => self::resolve_normal_key(input_mode, key),
        ServerInputMode::Resize => self::resolve_resize_key(input_mode, key),
    }
}

const fn resolve_normal_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match (&key.code, key.modifiers) {
        (ClientKeyCode::Char('E'), ClientKeyModifiers::SHIFT_ALT) => KeyResolution::Cmd(ClientCmd::Tab(TabCmd::Create)),
        (ClientKeyCode::Char('P'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::Tab(TabCmd::FocusPrevious))
        }
        (ClientKeyCode::Char('N'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::Tab(TabCmd::FocusNext))
        }
        (ClientKeyCode::Char('p'), ClientKeyModifiers::CTRL_ALT) => {
            KeyResolution::Cmd(ClientCmd::Tab(TabCmd::MovePrevious))
        }
        (ClientKeyCode::Char('n'), ClientKeyModifiers::CTRL_ALT) => {
            KeyResolution::Cmd(ClientCmd::Tab(TabCmd::MoveNext))
        }
        (ClientKeyCode::Char('H'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::FocusPane(PaneFocusDirection::Left))
        }
        (ClientKeyCode::Char('J'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::FocusPane(PaneFocusDirection::Down))
        }
        (ClientKeyCode::Char('K'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::FocusPane(PaneFocusDirection::Up))
        }
        (ClientKeyCode::Char('L'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::FocusPane(PaneFocusDirection::Right))
        }
        (ClientKeyCode::Char('V'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::SplitPane(PaneSplitAxis::Vertical))
        }
        (ClientKeyCode::Char('D'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::SplitPane(PaneSplitAxis::Horizontal))
        }
        (ClientKeyCode::Char('W'), ClientKeyModifiers::SHIFT_ALT) => KeyResolution::Cmd(ClientCmd::ClosePane),
        (ClientKeyCode::Char('R'), ClientKeyModifiers::SHIFT_ALT) => {
            *input_mode = ServerInputMode::Resize;
            KeyResolution::Cmd(ClientCmd::EnterResizeMode)
        }
        _ => KeyResolution::Raw,
    }
}

const fn resolve_resize_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match (&key.code, key.modifiers) {
        (ClientKeyCode::Esc, ClientKeyModifiers::NONE) => {
            *input_mode = ServerInputMode::Normal;
            KeyResolution::Cmd(ClientCmd::ExitMode)
        }
        (ClientKeyCode::Char('h') | ClientKeyCode::Left, ClientKeyModifiers::NONE) => {
            KeyResolution::Cmd(ClientCmd::ResizePane(PaneResizeDirection::Left))
        }
        (ClientKeyCode::Char('j') | ClientKeyCode::Down, ClientKeyModifiers::NONE) => {
            KeyResolution::Cmd(ClientCmd::ResizePane(PaneResizeDirection::Down))
        }
        (ClientKeyCode::Char('k') | ClientKeyCode::Up, ClientKeyModifiers::NONE) => {
            KeyResolution::Cmd(ClientCmd::ResizePane(PaneResizeDirection::Up))
        }
        (ClientKeyCode::Char('l') | ClientKeyCode::Right, ClientKeyModifiers::NONE) => {
            KeyResolution::Cmd(ClientCmd::ResizePane(PaneResizeDirection::Right))
        }
        _ => KeyResolution::Raw,
    }
}

/// Send one event on a pre-attach connection with the server's bounded write timeout.
///
/// # Errors
/// This function currently returns `Ok(false)` for send failures and timeouts instead of an error.
pub async fn send_connection_event_with_timeout(
    connection: &mut ServerConnection,
    event: &ServerEvent,
) -> rootcause::Result<bool> {
    match tokio::time::timeout(CLIENT_WRITE_TIMEOUT, connection.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}

/// Send one event on an attached-client writer with the server's bounded write timeout.
///
/// # Errors
/// This function currently returns `Ok(false)` for send failures and timeouts instead of an error.
pub async fn send_writer_event_with_timeout(
    writer: &mut ServerEventWriter,
    event: &ServerEvent,
) -> rootcause::Result<bool> {
    match tokio::time::timeout(CLIENT_WRITE_TIMEOUT, writer.send_event(event)).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(_)) | Err(_) => Ok(false),
    }
}

async fn join_client_tasks(handles: Vec<tokio::task::JoinHandle<rootcause::Result<()>>>) -> rootcause::Result<()> {
    for handle in handles {
        self::join_client_task(handle).await?;
    }
    Ok(())
}

async fn join_client_task(handle: tokio::task::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
    handle
        .await
        .unwrap_or_else(|error| Err(report!("muxr server client task panicked").attach(format!("{error}"))))
}

async fn join_finished_client_tasks(
    handles: &mut Vec<tokio::task::JoinHandle<rootcause::Result<()>>>,
) -> rootcause::Result<()> {
    let mut pending_handles = Vec::new();
    for handle in handles.drain(..) {
        if handle.is_finished() {
            self::join_client_task(handle).await?;
        } else {
            pending_handles.push(handle);
        }
    }
    *handles = pending_handles;
    Ok(())
}

fn run_async<T>(future: impl std::future::Future<Output = rootcause::Result<T>>) -> rootcause::Result<T> {
    tokio::runtime::Runtime::new()
        .context("failed to build muxr tokio runtime")?
        .block_on(future)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::thread;
    use std::time::Instant;

    use muxr_core::AttachRequest;
    use muxr_core::ClientKey;
    use muxr_core::ClientKeyCode;
    use muxr_core::ClientKeyModifiers;
    use muxr_core::ClientMousePosition;
    use muxr_core::RenderRowSpan;
    use muxr_core::RenderUpdate;
    use muxr_transport::ClientConnection;
    use muxr_transport::ClientEventReader;
    use muxr_transport::ClientRequestWriter;

    use super::*;
    use crate::pane_borders::PaneBorderAxis;
    use crate::pane_split::PaneSplitRatio;
    use crate::state::PaneTree;

    const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(2);
    const BALANCED_TEST_SPLIT_RATIO: u16 = 500;

    type PaneRegionTuple = (String, u16, u16, u16, u16);

    struct AttachedTestClient {
        layout: LayoutSnapshot,
        pane_regions: PaneRegionsSnapshot,
        reader: ClientEventReader,
        writer: ClientRequestWriter,
    }

    #[rstest::rstest]
    #[case::codex(ytil_agents::agent::Agent::Codex, "cx")]
    #[case::cursor(ytil_agents::agent::Agent::Cursor, "cu")]
    fn test_runtime_cmd_labels_when_agent_is_detected_uses_short_name(
        #[case] agent: ytil_agents::agent::Agent,
        #[case] expected_label: &str,
    ) -> rootcause::Result<()> {
        let pane_id = PaneId::new(1)?;

        pretty_assertions::assert_eq!(
            self::runtime_cmd_labels(&[PaneAgentDetection::Agent { pane_id, agent }]),
            vec![(pane_id, Some(expected_label.to_owned()))],
        );
        Ok(())
    }

    #[test]
    fn test_layout_snapshot_and_persist_when_runtime_cmd_exists_sets_snapshot_cmd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&session, self::metadata("zsh", 1))?;
        let runtimes = PaneRuntimes { panes: Vec::new() };
        let pane_id = PaneId::new(1)?;

        let snapshot = self::layout_snapshot_and_persist(
            &paths,
            &mut layout,
            &runtimes,
            &[(pane_id, Some("cx".to_owned()))],
            &[],
        )?;

        let pane = snapshot
            .tabs()
            .first()
            .and_then(|tab| tab.panes().first())
            .ok_or_else(|| report!("expected pane snapshot"))?;
        pretty_assertions::assert_eq!(pane.cmd_label, Some("cx".to_owned()));
        Ok(())
    }

    #[rstest::rstest]
    #[case::never(None, true)]
    #[case::recent(Some(Duration::from_millis(1)), false)]
    #[case::elapsed(Some(ATTENTION_POLL_INTERVAL), true)]
    fn test_detected_agents_refresh_due_when_last_refresh_varies(
        #[case] refreshed_age: Option<Duration>,
        #[case] expected: bool,
    ) {
        let now = Instant::now();
        let refreshed_at = refreshed_age.map(|age| now.checked_sub(age).expect("expected valid refresh age"));

        pretty_assertions::assert_eq!(self::detected_agents_refresh_due(refreshed_at, now), expected);
    }

    #[rstest::rstest]
    #[case::typing(b"abc", PaneUserInteraction::MayEcho)]
    #[case::enter(b"\r", PaneUserInteraction::StartsAgentWork)]
    #[case::input_with_newline(b"one\ntwo", PaneUserInteraction::StartsAgentWork)]
    fn test_input_interaction_when_bytes_vary_classifies_prompt_submission(
        #[case] bytes: &[u8],
        #[case] expected: PaneUserInteraction,
    ) {
        pretty_assertions::assert_eq!(self::input_interaction(bytes), expected);
    }

    #[test]
    fn test_serve_when_started_creates_session_root_socket_and_pid() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            self::make_public_session_dirs(&paths)?;
            let handle = self::spawn_test_server(&session, &paths, 1);

            self::wait_for_socket(&paths.socket)?;
            self::wait_for_path(&paths.layout)?;

            assert2::assert!(paths.root.is_dir());
            assert2::assert!(paths.panes.is_dir());
            assert2::assert!(paths.layout.exists());
            assert2::assert!(paths.socket.exists());
            assert2::assert!(paths.pid.exists());
            self::assert_session_paths_are_private(&paths)?;

            self::attach_and_detach(&session, &paths).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_disconnects_accepts_future_attach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            drop(self::open_attached_client(&session, &paths).await?);
            tokio::time::sleep(Duration::from_millis(25)).await;

            self::attach_and_detach(&session, &paths).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_reattached_accepts_second_attach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;

            self::attach_and_detach(&session, &paths).await?;
            self::attach_and_detach(&session, &paths).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_attached_reports_current_layout_snapshot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 1);

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::attach_request(&session)?).await?;
            let Some(ServerEvent::Attached(attached)) = connection.recv_event().await? else {
                return Err(report!("expected server attached response"));
            };

            pretty_assertions::assert_eq!(attached.layout.active_tab().to_string(), "tab-1");
            let Some(tab) = attached.layout.tabs().first() else {
                return Err(report!("expected one tab in layout snapshot"));
            };
            pretty_assertions::assert_eq!(tab.id().to_string(), "tab-1");
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-1");
            let Some(pane) = tab.panes().first() else {
                return Err(report!("expected one pane in layout snapshot"));
            };
            pretty_assertions::assert_eq!(pane.id.to_string(), "pane-1");

            connection.send_request(&ClientRequest::Detach).await?;
            self::read_connection_until_detached(&mut connection).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_second_client_attaches_rejects_it() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut first_client = self::open_attached_client(&session, &paths).await?;
            let mut second_client = self::connect_client(&paths).await?;

            second_client.send_request(&self::attach_request(&session)?).await?;
            let Some(ServerEvent::Error(error)) = second_client.recv_event().await? else {
                return Err(report!("expected second attach rejection"));
            };

            pretty_assertions::assert_eq!(error, ServerError::ClientAlreadyAttached);
            first_client.writer.send_request(&ClientRequest::Detach).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_never_sends_attach_request_does_not_occupy_attach_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let idle_client = self::connect_client(&paths).await?;

            self::attach_and_detach(&session, &paths).await?;

            drop(idle_client);
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_attached_client_does_not_answer_heartbeat_releases_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut stuck_client = self::connect_client(&paths).await?;
            stuck_client.send_request(&self::attach_request(&session)?).await?;
            tokio::time::sleep(
                CLIENT_HEARTBEAT_INTERVAL
                    + CLIENT_HEARTBEAT_TIMEOUT
                    + CLIENT_WRITE_TIMEOUT
                    + Duration::from_millis(100),
            )
            .await;

            let responsive_client = self::open_attached_client(&session, &paths).await?;
            self::detach_client(responsive_client).await?;

            drop(stuck_client);
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_ping_is_first_request_returns_pong_without_claiming_slot() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            let mut probe = self::connect_client(&paths).await?;
            probe.send_request(&ClientRequest::Ping).await?;
            pretty_assertions::assert_eq!(probe.recv_event().await?, Some(ServerEvent::Pong));

            self::attach_and_detach(&session, &paths).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_delete_session_is_first_request_stops_server_and_removes_state() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server(&session, &paths, 2);

            self::wait_for_socket(&paths.socket)?;
            self::wait_for_path(&paths.layout)?;
            let mut delete_client = self::connect_client(&paths).await?;

            delete_client.send_request(&ClientRequest::DeleteSession).await?;
            pretty_assertions::assert_eq!(delete_client.recv_event().await?, Some(ServerEvent::Deleted));
            self::join_server_with_timeout(handle)?;

            assert2::assert!(!paths.root.exists());
            assert2::assert!(!paths.socket.exists());
            assert2::assert!(!paths.pid.exists());
            Ok(())
        })
    }

    #[test]
    fn test_serve_when_delete_session_arrives_while_client_is_attached_removes_state() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, None, self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let _attached_client = self::open_attached_client(&session, &paths).await?;
            let mut delete_client = self::connect_client(&paths).await?;

            delete_client.send_request(&ClientRequest::DeleteSession).await?;
            pretty_assertions::assert_eq!(delete_client.recv_event().await?, Some(ServerEvent::Deleted));
            self::join_server_with_timeout(handle)?;

            assert2::assert!(!paths.root.exists());
            assert2::assert!(!paths.socket.exists());
            assert2::assert!(!paths.pid.exists());
            Ok(())
        })
    }

    #[test]
    fn test_layout_tab_cmds_when_tabs_exist_mutates_active_tab_and_order() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        layout.create_tab(self::metadata("sh", 2))?;
        layout.create_tab(self::metadata("sh", 3))?;
        pretty_assertions::assert_eq!(self::layout_tab_ids(&layout)?, vec!["tab-1", "tab-2", "tab-3"]);
        pretty_assertions::assert_eq!(layout.active_tab_id().to_string(), "tab-3");

        layout.focus_previous_tab()?;
        pretty_assertions::assert_eq!(layout.active_tab_id().to_string(), "tab-2");
        layout.move_active_tab_previous()?;
        pretty_assertions::assert_eq!(self::layout_tab_ids(&layout)?, vec!["tab-2", "tab-1", "tab-3"]);
        pretty_assertions::assert_eq!(layout.active_tab_id().to_string(), "tab-2");
        layout.move_active_tab_next()?;
        pretty_assertions::assert_eq!(self::layout_tab_ids(&layout)?, vec!["tab-1", "tab-2", "tab-3"]);
        layout.focus_next_tab()?;
        pretty_assertions::assert_eq!(layout.active_tab_id().to_string(), "tab-3");
        Ok(())
    }

    #[test]
    fn test_layout_split_and_close_when_multiple_panes_updates_active_pane() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        let pane_id = layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;

        pretty_assertions::assert_eq!(pane_id.to_string(), "pane-2");
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        pretty_assertions::assert_eq!(self::layout_active_tab_pane_ids(&layout)?, vec!["pane-1", "pane-2"]);

        let close = layout.close_active_pane(3)?;

        pretty_assertions::assert_eq!(
            close,
            ClosePaneOutcome::Removed {
                pane_id: PaneId::new(2)?,
            },
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-1");
        pretty_assertions::assert_eq!(self::layout_active_tab_pane_ids(&layout)?, vec!["pane-1"]);
        Ok(())
    }

    #[test]
    fn test_active_pane_session_metadata_when_cwd_was_synced_uses_active_pane_cwd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let active_cwd = tempfile::tempdir()?;
        let active_cwd = active_cwd.path().to_string_lossy().into_owned();
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.sync_terminal_titles(&[(PaneId::new(1)?, Some(active_cwd.clone()))]);

        let metadata = self::active_pane_session_metadata(&config, &layout)?;

        pretty_assertions::assert_eq!(metadata.cwd, active_cwd);
        Ok(())
    }

    #[test]
    fn test_layout_split_active_pane_when_cwd_was_synced_new_pane_inherits_active_pane_cwd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let active_cwd = tempfile::tempdir()?;
        let active_cwd = active_cwd.path().to_string_lossy().into_owned();
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.sync_terminal_titles(&[(PaneId::new(1)?, Some(active_cwd.clone()))]);

        let pane_id = layout.split_active_pane(
            self::active_pane_session_metadata(&config, &layout)?,
            PaneSplitAxis::Vertical,
        )?;

        let pane = layout
            .pane(pane_id)
            .ok_or_else(|| report!("expected split pane to exist").attach(format!("pane_id={pane_id}")))?;
        pretty_assertions::assert_eq!(pane.cwd, active_cwd);
        Ok(())
    }

    #[test]
    fn test_layout_create_tab_when_cwd_was_synced_new_pane_inherits_active_pane_cwd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let active_cwd = tempfile::tempdir()?;
        let active_cwd = active_cwd.path().to_string_lossy().into_owned();
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.sync_terminal_titles(&[(PaneId::new(1)?, Some(active_cwd.clone()))]);

        let pane_id = layout.create_tab(self::active_pane_session_metadata(&config, &layout)?)?;

        let pane = layout
            .pane(pane_id)
            .ok_or_else(|| report!("expected tab pane to exist").attach(format!("pane_id={pane_id}")))?;
        pretty_assertions::assert_eq!(pane.cwd, active_cwd);
        Ok(())
    }

    #[test]
    fn test_handle_close_pane_cmd_when_title_cwd_is_pending_persists_synced_cwd() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = self::server_config(tempdir.path(), "work")?;
        config.shell_cmd = self::shell_cmd("/bin/cat");
        fs::create_dir_all(&config.paths.root).context("failed to create muxr test session root")?;
        let layout = Mutex::new(SessionLayout::initial(&config.session, self::metadata("sh", 1))?);
        let terminal_size = TerminalSize::new(80, 24)?;
        let runtimes = {
            let layout = self::lock_mutex(&layout, "layout")?;
            PaneRuntimes::spawn_for_layout(&config, &layout, &terminal_size)?
        };
        let runtimes = Mutex::new(runtimes);
        let pane_id = PaneId::new(1)?;
        {
            let runtimes = self::lock_mutex(&runtimes, "pane runtimes")?;
            let handle = runtimes.handle(pane_id)?;
            drop(runtimes);
            let _scrolled_to_bottom = handle.write_input(b"\x1b]2;~\x07\n")?;
        }

        let started_at = Instant::now();
        loop {
            let title = {
                let runtimes = self::lock_mutex(&runtimes, "pane runtimes")?;
                runtimes.handle(pane_id)?.terminal_title()?
            };
            if title.as_deref() == Some("~") {
                break;
            }
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr test terminal title"));
            }
            thread::sleep(Duration::from_millis(10));
        }

        let outcome = crate::pane_close::handle_close_pane_cmd(&config, &layout, &runtimes)?;

        pretty_assertions::assert_eq!(outcome, ClosePaneOutcome::Final { pane_id });
        let persisted = crate::state::persisted::load_metadata(&config.paths, &config.session)?
            .ok_or_else(|| report!("expected muxr layout metadata"))?;
        let pane = persisted
            .pane(pane_id)
            .ok_or_else(|| report!("expected persisted muxr pane").attach(format!("pane_id={pane_id}")))?;
        pretty_assertions::assert_eq!(pane.cwd, "~");
        Ok(())
    }

    #[test]
    fn test_initial_attached_render_when_detached_output_arrived_does_not_mark_attention() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = self::server_config(tempdir.path(), "work")?;
        config.shell_cmd = self::shell_cmd("/bin/sh").arg("-c").arg("printf dirty; sleep 30");
        let terminal_size = TerminalSize::new(80, 24)?;
        let layout = Mutex::new(SessionLayout::initial(&config.session, self::metadata("sh", 1))?);
        {
            let mut layout = self::lock_mutex(&layout, "layout")?;
            layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        }
        let runtimes = {
            let layout = self::lock_mutex(&layout, "layout")?;
            PaneRuntimes::spawn_for_layout(&config, &layout, &terminal_size)?
        };
        let runtimes = Mutex::new(runtimes);
        let inactive_pane = PaneId::new(1)?;
        let active_pane = PaneId::new(2)?;
        self::wait_for_runtime_snapshot_contains(&runtimes, inactive_pane, "dirty")?;
        self::wait_for_runtime_snapshot_contains(&runtimes, active_pane, "dirty")?;

        self::resize_panes_to_layout(&layout, &runtimes, &terminal_size)?;
        drop(self::initial_attached_render(
            &config.paths,
            &layout,
            &runtimes,
            &terminal_size,
        )?);

        let layout = self::lock_mutex(&layout, "layout")?;
        pretty_assertions::assert_eq!(layout.attention_pane_ids(), Vec::<PaneId>::new());
        Ok(())
    }

    #[test]
    fn test_handle_create_tab_when_pane_spawn_fails_restores_layout() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = self::server_config(tempdir.path(), "work")?;
        config.shell_cmd = self::shell_cmd("/bin/muxr-missing-shell");
        let initial_layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        let layout = Mutex::new(initial_layout.clone());
        let runtimes = Mutex::new(PaneRuntimes { panes: Vec::new() });

        let create_result = {
            let mut layout = self::lock_mutex(&layout, "layout")?;
            crate::tab_create::handle_create_tab(&mut layout, &config, &runtimes, &TerminalSize::new(80, 24)?)
        };
        assert2::assert!(create_result.is_err());

        let layout = self::lock_mutex(&layout, "layout")?;
        pretty_assertions::assert_eq!(*layout, initial_layout);
        pretty_assertions::assert_eq!(self::lock_mutex(&runtimes, "pane runtimes")?.panes.len(), 0);
        assert2::assert!(!config.paths.layout.exists());
        Ok(())
    }

    #[test]
    fn test_handle_split_pane_cmd_when_pane_spawn_fails_restores_layout() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let mut config = self::server_config(tempdir.path(), "work")?;
        config.shell_cmd = self::shell_cmd("/bin/muxr-missing-shell");
        let initial_layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        let layout = Mutex::new(initial_layout.clone());
        let runtimes = Mutex::new(PaneRuntimes { panes: Vec::new() });

        assert2::assert!(
            crate::pane_split::handle_split_pane_cmd(
                PaneSplitAxis::Vertical,
                &config,
                &layout,
                &runtimes,
                &TerminalSize::new(80, 24)?,
            )
            .is_err()
        );

        let layout = self::lock_mutex(&layout, "layout")?;
        pretty_assertions::assert_eq!(*layout, initial_layout);
        pretty_assertions::assert_eq!(self::lock_mutex(&runtimes, "pane runtimes")?.panes.len(), 0);
        assert2::assert!(!config.paths.layout.exists());
        Ok(())
    }

    #[rstest::rstest]
    #[case::first_pane(ClientMousePosition { row: 0, col: 0 }, "pane-1", true)]
    #[case::border(ClientMousePosition { row: 0, col: 40 }, "pane-2", false)]
    #[case::second_pane(ClientMousePosition { row: 0, col: 41 }, "pane-2", false)]
    fn test_layout_focus_pane_at_when_mouse_position_arrives_updates_active_pane(
        #[case] position: ClientMousePosition,
        #[case] expected_active_pane: &str,
        #[case] expected_changed: bool,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        self::force_balanced_test_split_ratio(&mut layout)?;

        pretty_assertions::assert_eq!(
            layout.focus_pane_at(&TerminalSize::new(80, 24)?, position)?,
            expected_changed,
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), expected_active_pane);
        Ok(())
    }

    #[rstest::rstest]
    #[case::first_pane(ClientMousePosition { row: 0, col: 0 }, Some("pane-1"))]
    #[case::border(ClientMousePosition { row: 0, col: 40 }, None)]
    #[case::second_pane(ClientMousePosition { row: 0, col: 41 }, Some("pane-2"))]
    fn test_layout_pane_at_when_mouse_position_arrives_returns_pane_without_focus_change(
        #[case] position: ClientMousePosition,
        #[case] expected_pane: Option<&str>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        self::force_balanced_test_split_ratio(&mut layout)?;

        let pane_id = layout.pane_at(&TerminalSize::new(80, 24)?, position)?;

        pretty_assertions::assert_eq!(pane_id.map(|id| id.to_string()), expected_pane.map(str::to_owned));
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        Ok(())
    }

    #[rstest::rstest]
    #[case::left(PaneFocusDirection::Left, "pane-1", true)]
    #[case::right_edge(PaneFocusDirection::Right, "pane-2", false)]
    #[case::up_edge(PaneFocusDirection::Up, "pane-2", false)]
    #[case::down_edge(PaneFocusDirection::Down, "pane-2", false)]
    fn test_layout_focus_pane_direction_when_adjacent_pane_exists_updates_active_pane(
        #[case] direction: PaneFocusDirection,
        #[case] expected_active_pane: &str,
        #[case] expected_changed: bool,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;

        pretty_assertions::assert_eq!(
            layout.focus_pane_direction(&TerminalSize::new(80, 24)?, direction)?,
            expected_changed,
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), expected_active_pane);
        Ok(())
    }

    #[test]
    fn test_layout_focus_pane_direction_when_multiple_adjacent_panes_exist_uses_recent_focus() -> rootcause::Result<()>
    {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(self::metadata("sh", 3), PaneSplitAxis::Horizontal)?;

        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Up)?);
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Left)?);
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-1");

        assert2::assert!(layout.focus_pane_direction(&TerminalSize::new(80, 24)?, PaneFocusDirection::Right)?);

        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical_then_horizontal(
        PaneSplitAxis::Vertical,
        PaneSplitAxis::Horizontal,
        vec![
            ("pane-1", 0, 0, 40, 24),
            ("pane-2", 41, 0, 39, 12),
            ("pane-3", 41, 13, 39, 11),
        ],
    )]
    #[case::horizontal_then_vertical(
        PaneSplitAxis::Horizontal,
        PaneSplitAxis::Vertical,
        vec![
            ("pane-1", 0, 0, 80, 12),
            ("pane-2", 0, 13, 40, 11),
            ("pane-3", 41, 13, 39, 11),
        ],
    )]
    fn test_layout_split_when_nested_splits_only_active_pane(
        #[case] first_axis: PaneSplitAxis,
        #[case] second_axis: PaneSplitAxis,
        #[case] expected_regions: Vec<(&str, u16, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), first_axis)?;
        layout.split_active_pane(self::metadata("sh", 3), second_axis)?;
        self::force_balanced_test_split_ratio(&mut layout)?;

        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-3");
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_ids(&layout)?,
            vec!["pane-1", "pane-2", "pane-3"]
        );
        let expected_regions = expected_regions
            .into_iter()
            .map(|(id, col, row, cols, rows)| (id.to_owned(), col, row, cols, rows))
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            expected_regions
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical(
        PaneSplitAxis::Vertical,
        vec![(PaneBorderAxis::Vertical, 40, 0, 24)],
    )]
    #[case::horizontal(
        PaneSplitAxis::Horizontal,
        vec![(PaneBorderAxis::Horizontal, 0, 12, 80)],
    )]
    fn test_layout_split_when_split_exists_reserves_border_cell(
        #[case] split_axis: PaneSplitAxis,
        #[case] expected_borders: Vec<(PaneBorderAxis, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), split_axis)?;
        self::force_balanced_test_split_ratio(&mut layout)?;

        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_borders(&layout, &TerminalSize::new(80, 24)?)?,
            expected_borders
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::dirty_frame(RenderDiffReason::DirtyFrame, false)]
    #[case::region_changed(RenderDiffReason::RegionChanged, true)]
    fn test_render_composer_render_frame_diff_when_pixels_are_unchanged_respects_reason(
        #[case] reason: RenderDiffReason,
        #[case] expected_diff: bool,
    ) -> rootcause::Result<()> {
        let size = TerminalSize::new(2, 1)?;
        let cursor = RenderCursor {
            row: 0,
            col: 0,
            visible: false,
        };
        let rows = vec![RenderRowSpan::new(
            0,
            0,
            vec![
                RenderCell::narrow("a", RenderStyle::default()),
                RenderCell::narrow("b", RenderStyle::default()),
            ],
        )?];
        let previous = CompositeFrame {
            cursor: cursor.clone(),
            rows: rows.clone(),
            seq: 1,
            size: size.clone(),
        };
        let current = CompositeFrame {
            cursor,
            rows,
            seq: 0,
            size,
        };
        let mut composer = RenderComposer {
            last_sent: Some(previous),
            next_seq: 2,
        };

        let update = composer.render_frame_diff(current, reason)?;

        if !expected_diff {
            pretty_assertions::assert_eq!(update, None);
            pretty_assertions::assert_eq!(composer.next_seq, 2);
            return Ok(());
        }

        let Some(RenderUpdate::Diff(diff)) = update else {
            return Err(report!("expected muxr region-change diff"));
        };
        pretty_assertions::assert_eq!(diff.base_seq(), 1);
        pretty_assertions::assert_eq!(diff.seq(), 2);
        assert2::assert!(diff.rows().is_empty());
        Ok(())
    }

    #[test]
    fn test_paste_snapshot_when_visible_url_is_present_adds_hyperlink_metadata() -> rootcause::Result<()> {
        let size = TerminalSize::new(24, 1)?;
        let mut terminal = crate::terminal::TerminalState::new(&size);
        let _ = terminal.process(b"https://example.com");
        let snapshot = terminal.snapshot()?;
        let region = PaneRegion {
            area: crate::pane_layout::PaneArea {
                origin: crate::pane_layout::PanePosition { row: 0, col: 0 },
                size: crate::pane_layout::PaneSize { rows: 1, cols: 24 },
            },
            focus_seq: 1,
            id: PaneId::new(1)?,
        };
        let mut rows = self::empty_render_rows(&size);

        self::paste_snapshot(&mut rows, &region, &snapshot)?;

        let row = rows.first().ok_or_else(|| report!("expected muxr composite row"))?;
        let linked_cells = row.iter().filter(|cell| cell.hyperlink().is_some()).collect::<Vec<_>>();
        let linked_text = linked_cells.iter().map(|cell| cell.text()).collect::<String>();
        pretty_assertions::assert_eq!(linked_text, "https://example.com");
        for cell in linked_cells {
            pretty_assertions::assert_eq!(
                cell.hyperlink().map(muxr_core::RenderHyperlink::uri),
                Some("https://example.com")
            );
        }
        Ok(())
    }

    #[test]
    fn test_layout_close_when_nested_pane_closes_collapses_parent_split() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(self::metadata("sh", 3), PaneSplitAxis::Horizontal)?;
        self::force_balanced_test_split_ratio(&mut layout)?;
        let close = layout.close_active_pane(3)?;

        pretty_assertions::assert_eq!(
            close,
            ClosePaneOutcome::Removed {
                pane_id: PaneId::new(3)?,
            },
        );
        pretty_assertions::assert_eq!(layout.active_pane_id()?.to_string(), "pane-2");
        pretty_assertions::assert_eq!(self::layout_active_tab_pane_ids(&layout)?, vec!["pane-1", "pane-2"]);
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 24),
            ],
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical_left(
        PaneSplitAxis::Vertical,
        PaneResizeDirection::Left,
        vec![
            ("pane-1", 0, 0, 36, 24),
            ("pane-2", 37, 0, 43, 24),
        ],
    )]
    #[case::vertical_right(
        PaneSplitAxis::Vertical,
        PaneResizeDirection::Right,
        vec![
            ("pane-1", 0, 0, 43, 24),
            ("pane-2", 44, 0, 36, 24),
        ],
    )]
    #[case::horizontal_up(
        PaneSplitAxis::Horizontal,
        PaneResizeDirection::Up,
        vec![
            ("pane-1", 0, 0, 80, 10),
            ("pane-2", 0, 11, 80, 13),
        ],
    )]
    #[case::horizontal_down(
        PaneSplitAxis::Horizontal,
        PaneResizeDirection::Down,
        vec![
            ("pane-1", 0, 0, 80, 13),
            ("pane-2", 0, 14, 80, 10),
        ],
    )]
    fn test_layout_resize_active_pane_when_resize_cmd_arrives_updates_geometry(
        #[case] split_axis: PaneSplitAxis,
        #[case] direction: PaneResizeDirection,
        #[case] expected_regions: Vec<(&str, u16, u16, u16, u16)>,
    ) -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), split_axis)?;
        self::force_balanced_test_split_ratio(&mut layout)?;

        assert2::assert!(layout.resize_active_pane(direction)?);
        let expected_regions = expected_regions
            .into_iter()
            .map(|(id, col, row, cols, rows)| (id.to_owned(), col, row, cols, rows))
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            expected_regions
        );
        Ok(())
    }

    #[test]
    fn test_layout_resize_nested_splits_resizes_nearest_matching_axis() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(self::metadata("sh", 3), PaneSplitAxis::Horizontal)?;
        self::force_balanced_test_split_ratio(&mut layout)?;

        assert2::assert!(layout.resize_active_pane(PaneResizeDirection::Up)?);
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 10),
                ("pane-3".to_owned(), 41, 11, 39, 13),
            ],
        );

        assert2::assert!(layout.resize_active_pane(PaneResizeDirection::Left)?);
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&layout, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 36, 24),
                ("pane-2".to_owned(), 37, 0, 43, 10),
                ("pane-3".to_owned(), 37, 11, 43, 13),
            ],
        );
        Ok(())
    }

    #[test]
    fn test_layout_metadata_when_nested_panes_exist_round_trips_tree() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        fs::create_dir_all(&config.paths.root)?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        layout.split_active_pane(self::metadata("sh", 3), PaneSplitAxis::Horizontal)?;
        self::force_balanced_test_split_ratio(&mut layout)?;
        crate::state::persisted::write_metadata(&config.paths, &layout)?;

        let loaded = crate::state::persisted::load_metadata(&config.paths, &config.session)?
            .ok_or_else(|| report!("expected muxr layout metadata to load"))?;

        pretty_assertions::assert_eq!(loaded.active_pane_id()?.to_string(), "pane-3");
        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&loaded, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 40, 24),
                ("pane-2".to_owned(), 41, 0, 39, 12),
                ("pane-3".to_owned(), 41, 13, 39, 11),
            ],
        );
        Ok(())
    }

    #[test]
    fn test_layout_metadata_when_resized_panes_exist_round_trips_split_ratio() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let config = self::server_config(tempdir.path(), "work")?;
        fs::create_dir_all(&config.paths.root)?;
        let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), PaneSplitAxis::Vertical)?;
        self::force_balanced_test_split_ratio(&mut layout)?;
        assert2::assert!(layout.resize_active_pane(PaneResizeDirection::Left)?);
        crate::state::persisted::write_metadata(&config.paths, &layout)?;

        let loaded = crate::state::persisted::load_metadata(&config.paths, &config.session)?
            .ok_or_else(|| report!("expected muxr layout metadata to load"))?;

        pretty_assertions::assert_eq!(
            self::layout_active_tab_pane_regions(&loaded, &TerminalSize::new(80, 24)?)?,
            vec![
                ("pane-1".to_owned(), 0, 0, 36, 24),
                ("pane-2".to_owned(), 37, 0, 43, 24),
            ],
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::create_tab(
        ClientKeyCode::Char('E'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bE",
        ClientCmd::Tab(TabCmd::Create)
    )]
    #[case::focus_previous_tab(
        ClientKeyCode::Char('P'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bP",
        ClientCmd::Tab(TabCmd::FocusPrevious)
    )]
    #[case::focus_next_tab(
        ClientKeyCode::Char('N'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bN",
        ClientCmd::Tab(TabCmd::FocusNext)
    )]
    #[case::move_tab_previous(
        ClientKeyCode::Char('p'),
        ClientKeyModifiers::CTRL_ALT,
        b"\x1b\x10",
        ClientCmd::Tab(TabCmd::MovePrevious)
    )]
    #[case::move_tab_next(
        ClientKeyCode::Char('n'),
        ClientKeyModifiers::CTRL_ALT,
        b"\x1b\x0e",
        ClientCmd::Tab(TabCmd::MoveNext)
    )]
    #[case::focus_pane_left(
        ClientKeyCode::Char('H'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bH",
        ClientCmd::FocusPane(PaneFocusDirection::Left)
    )]
    #[case::focus_pane_down(
        ClientKeyCode::Char('J'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bJ",
        ClientCmd::FocusPane(PaneFocusDirection::Down)
    )]
    #[case::focus_pane_up(
        ClientKeyCode::Char('K'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bK",
        ClientCmd::FocusPane(PaneFocusDirection::Up)
    )]
    #[case::focus_pane_right(
        ClientKeyCode::Char('L'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bL",
        ClientCmd::FocusPane(PaneFocusDirection::Right)
    )]
    #[case::split_pane_vertical(
        ClientKeyCode::Char('V'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bV",
        ClientCmd::SplitPane(PaneSplitAxis::Vertical)
    )]
    #[case::split_pane_horizontal(
        ClientKeyCode::Char('D'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bD",
        ClientCmd::SplitPane(PaneSplitAxis::Horizontal)
    )]
    #[case::close_pane(
        ClientKeyCode::Char('W'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bW",
        ClientCmd::ClosePane
    )]
    fn test_resolve_key_when_normal_bound_key_arrives_returns_cmd(
        #[case] code: ClientKeyCode,
        #[case] modifiers: ClientKeyModifiers,
        #[case] raw_bytes: &[u8],
        #[case] cmd: ClientCmd,
    ) {
        let mut input_mode = ServerInputMode::Normal;
        let key = ClientKey {
            code,
            modifiers,
            raw_bytes: raw_bytes.to_vec(),
        };

        pretty_assertions::assert_eq!(resolve_key(&mut input_mode, &key), KeyResolution::Cmd(cmd),);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    #[test]
    fn test_resolve_key_when_unbound_key_arrives_returns_raw() {
        let mut input_mode = ServerInputMode::Normal;
        let key = ClientKey {
            code: ClientKeyCode::Char('x'),
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: b"x".to_vec(),
        };

        pretty_assertions::assert_eq!(resolve_key(&mut input_mode, &key), KeyResolution::Raw);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    #[rstest::rstest]
    #[case::left(ClientKeyCode::Char('h'), ClientCmd::ResizePane(PaneResizeDirection::Left))]
    #[case::down(ClientKeyCode::Char('j'), ClientCmd::ResizePane(PaneResizeDirection::Down))]
    #[case::up(ClientKeyCode::Char('k'), ClientCmd::ResizePane(PaneResizeDirection::Up))]
    #[case::right(ClientKeyCode::Char('l'), ClientCmd::ResizePane(PaneResizeDirection::Right))]
    #[case::arrow_left(ClientKeyCode::Left, ClientCmd::ResizePane(PaneResizeDirection::Left))]
    #[case::arrow_down(ClientKeyCode::Down, ClientCmd::ResizePane(PaneResizeDirection::Down))]
    #[case::arrow_up(ClientKeyCode::Up, ClientCmd::ResizePane(PaneResizeDirection::Up))]
    #[case::arrow_right(ClientKeyCode::Right, ClientCmd::ResizePane(PaneResizeDirection::Right))]
    fn test_resolve_key_when_resize_mode_key_arrives_returns_resize_cmd(
        #[case] code: ClientKeyCode,
        #[case] cmd: ClientCmd,
    ) {
        let mut input_mode = ServerInputMode::Resize;
        let key = ClientKey {
            code,
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: b"x".to_vec(),
        };

        pretty_assertions::assert_eq!(resolve_key(&mut input_mode, &key), KeyResolution::Cmd(cmd),);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Resize);
    }

    #[test]
    fn test_resolve_key_when_resize_mode_enter_and_exit_arrive_updates_server_mode() {
        let mut input_mode = ServerInputMode::Normal;
        let enter = ClientKey {
            code: ClientKeyCode::Char('R'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: b"\x1bR".to_vec(),
        };
        let exit = ClientKey {
            code: ClientKeyCode::Esc,
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: b"\x1b".to_vec(),
        };

        pretty_assertions::assert_eq!(
            resolve_key(&mut input_mode, &enter),
            KeyResolution::Cmd(ClientCmd::EnterResizeMode),
        );
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Resize);
        pretty_assertions::assert_eq!(
            resolve_key(&mut input_mode, &exit),
            KeyResolution::Cmd(ClientCmd::ExitMode),
        );
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    #[test]
    fn test_serve_when_key_request_arrives_writes_raw_bytes_and_stays_attached() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('x'),
                    modifiers: ClientKeyModifiers::NONE,
                    raw_bytes: b"x\n".to_vec(),
                }))
                .await?;

            self::read_until_render_contains(&mut client, b"x").await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_terminal_title_reports_cwd_persists_metadata() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Input(b"\x1b]2;~\x07\n".to_vec()))
                .await?;

            let layout = self::read_until_sidebar_layout(&mut client).await?;
            let Some(tab) = layout.tabs().first() else {
                return Err(report!("expected muxr test layout tab"));
            };
            let Some(pane) = tab.panes().first() else {
                return Err(report!("expected muxr test layout pane"));
            };
            pretty_assertions::assert_eq!(pane.cwd, "~");
            pretty_assertions::assert_eq!(pane.cmd_label, None);

            let persisted = crate::state::persisted::load_metadata(&paths, &session)?
                .ok_or_else(|| report!("expected muxr layout metadata"))?;
            let pane = persisted
                .pane(PaneId::new(1)?)
                .ok_or_else(|| report!("expected persisted muxr pane"))?;
            pretty_assertions::assert_eq!(pane.cwd, "~");

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_create_tab_key_arrives_sends_layout_and_persists_metadata() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('E'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bE".to_vec(),
                }))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            pretty_assertions::assert_eq!(layout.active_tab().to_string(), "tab-2");
            pretty_assertions::assert_eq!(
                layout.tabs().iter().map(|tab| tab.id().to_string()).collect::<Vec<_>>(),
                vec!["tab-1", "tab-2"],
            );
            self::assert_layout_metadata_tabs(&paths, &[1, 2], 2)?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_layout_metadata_exists_restores_tab_order_on_attach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let config = self::server_config(tempdir.path(), "work")?;
            fs::create_dir_all(&config.paths.root)?;
            let mut layout = SessionLayout::initial(&config.session, self::metadata("sh", 1))?;
            layout.create_tab(self::metadata("sh", 2))?;
            crate::state::persisted::write_metadata(&config.paths, &layout)?;
            let paths = config.paths.clone();
            let session = config.session.clone();
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let client = self::open_attached_client(&session, &paths).await?;
            pretty_assertions::assert_eq!(client.layout.active_tab().to_string(), "tab-2");
            pretty_assertions::assert_eq!(
                client
                    .layout
                    .tabs()
                    .iter()
                    .map(|tab| tab.id().to_string())
                    .collect::<Vec<_>>(),
                vec!["tab-1", "tab-2"],
            );
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_split_pane_key_arrives_sends_layout_and_routes_input_to_new_pane() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected tab after split"))?;
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-2");
            pretty_assertions::assert_eq!(
                tab.panes().iter().map(|pane| pane.id.to_string()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );

            client
                .writer
                .send_request(&ClientRequest::Input(b"new-pane\n".to_vec()))
                .await?;
            self::read_until_render_contains(&mut client, b"new-pane").await?;
            self::assert_layout_metadata_panes(&paths, &[1, 2], 2)?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_no_button_mouse_motion_arrives_does_not_focus_hovered_pane() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            drop(self::read_until_layout(&mut client).await?);

            client
                .writer
                .send_request(&ClientRequest::Mouse(ClientMouseEvent {
                    button: 35,
                    phase: muxr_core::ClientMouseEventPhase::Press,
                    position: muxr_core::ClientMousePosition { row: 0, col: 0 },
                }))
                .await?;
            client
                .writer
                .send_request(&ClientRequest::Input(b"still-pane-2\n".to_vec()))
                .await?;

            self::read_until_render_contains(&mut client, b"still-pane-2").await?;
            self::assert_layout_metadata_panes(&paths, &[1, 2], 2)?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_wheel_over_inactive_plain_pane_scrolls_pointed_history_without_focus_change()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd("/bin/sh")
                    .arg("-c")
                    .arg("i=0; while [ $i -lt 80 ]; do printf 'line-%02d\\n' \"$i\"; i=$((i + 1)); done; sleep 30"),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut client, b"line-79").await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected tab after split"))?;
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-2");
            self::read_until_render_contains(&mut client, b"line-79").await?;
            let ready_regions =
                self::read_until_pane_regions_matching(&mut client, "both panes have scrollback", |regions| {
                    Ok(self::pane_region(regions, "pane-1")?.visible_top_row() > 0
                        && self::pane_region(regions, "pane-2")?.visible_top_row() > 0)
                })
                .await?;
            let pane_1_before = self::pane_region(&ready_regions, "pane-1")?.visible_top_row();
            let pane_2_before = self::pane_region(&ready_regions, "pane-2")?.visible_top_row();

            client
                .writer
                .send_request(&ClientRequest::Mouse(ClientMouseEvent {
                    button: 64,
                    phase: muxr_core::ClientMouseEventPhase::Press,
                    position: self::pane_position(&ready_regions, "pane-1")?,
                }))
                .await?;

            let scrolled_regions =
                self::read_until_pane_regions_matching(&mut client, "pointed pane scrollback moved", |regions| {
                    Ok(self::pane_region(regions, "pane-1")?.visible_top_row() < pane_1_before
                        && self::pane_region(regions, "pane-2")?.visible_top_row() == pane_2_before)
                })
                .await?;
            pretty_assertions::assert_eq!(
                self::pane_region(&scrolled_regions, "pane-2")?.visible_top_row(),
                pane_2_before,
            );
            self::assert_layout_metadata_panes(&paths, &[1, 2], 2)?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_selection_line_scroll_cannot_move_sends_noop_scroll_result() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            let position = ClientMousePosition { row: 0, col: 0 };
            client
                .writer
                .send_request(&ClientRequest::ScrollPaneLineAt {
                    position,
                    direction: PaneScrollDirection::Down,
                })
                .await?;

            pretty_assertions::assert_eq!(
                self::read_until_scroll_pane_line_result(&mut client).await?,
                (position, PaneScrollDirection::Down, false),
            );
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_wheel_over_app_mouse_pane_forwards_mouse_bytes_to_pty() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd("/bin/sh").arg("-c").arg(
                    "printf '\\033[?1002h\\033[?1006hready\\n'; \
                     stty raw -echo; \
                     dd bs=1 count=10 2>/dev/null | od -An -tx1 -v; \
                     sleep 30",
                ),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut client, b"ready").await?;
            let regions = self::read_until_pane_regions_matching(&mut client, "app mouse mode enabled", |regions| {
                Ok(self::pane_region(regions, "pane-1")?.mouse_mode() == muxr_core::PaneMouseMode::ButtonMotion)
            })
            .await?;

            client
                .writer
                .send_request(&ClientRequest::Mouse(ClientMouseEvent {
                    button: 64,
                    phase: muxr_core::ClientMouseEventPhase::Press,
                    position: self::pane_position(&regions, "pane-1")?,
                }))
                .await?;

            self::read_until_render_contains_hex_bytes(
                &mut client,
                &["1b", "5b", "3c", "36", "34", "3b", "31", "3b", "31", "4d"],
            )
            .await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_wheel_over_alternate_screen_without_mouse_protocol_sends_faux_scroll_input()
    -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd("/bin/sh").arg("-c").arg(
                    "printf '\\033[?1049hready\\n'; \
                     stty raw -echo; \
                     dd bs=1 count=9 2>/dev/null | od -An -tx1 -v; \
                     sleep 30",
                ),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut client, b"ready").await?;

            client
                .writer
                .send_request(&ClientRequest::Mouse(ClientMouseEvent {
                    button: 64,
                    phase: muxr_core::ClientMouseEventPhase::Press,
                    position: self::pane_position(&client.pane_regions, "pane-1")?,
                }))
                .await?;

            self::read_until_render_contains_hex_bytes(
                &mut client,
                &["1b", "5b", "41", "1b", "5b", "41", "1b", "5b", "41"],
            )
            .await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_close_pane_key_arrives_removes_active_pane_and_keeps_remaining_pty() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            drop(self::read_until_layout(&mut client).await?);

            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('W'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bW".to_vec(),
                }))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected tab after close"))?;
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-1");
            pretty_assertions::assert_eq!(
                tab.panes().iter().map(|pane| pane.id.to_string()).collect::<Vec<_>>(),
                vec!["pane-1"],
            );

            client
                .writer
                .send_request(&ClientRequest::Input(b"remaining\n".to_vec()))
                .await?;
            self::read_until_render_contains(&mut client, b"remaining").await?;
            self::assert_layout_metadata_panes(&paths, &[1], 1)?;

            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_final_pane_is_closed_persists_and_exits() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));

        self::runtime()?.block_on(async {
            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('W'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bW".to_vec(),
                }))
                .await?;
            self::read_client_until_detached(&mut client).await?;
            drop(client);
            Ok::<(), rootcause::Report>(())
        })?;

        self::join_server_with_timeout(handle)?;
        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        self::assert_final_closed_layout_metadata(&paths)?;
        Ok(())
    }

    #[test]
    fn test_serve_resize_mode_sequence_resizes_and_escape_exits_mode() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(&session, &paths, Some(1), self::shell_cmd("/bin/cat"));
            let size = TerminalSize::new(80, 24)?;

            self::wait_for_socket(&paths.socket)?;
            let mut client = self::open_attached_client(&session, &paths).await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('V'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bV".to_vec(),
                }))
                .await?;
            drop(self::read_until_layout(&mut client).await?);
            let before_resize = crate::state::persisted::load_metadata(&paths, &session)?
                .ok_or_else(|| report!("expected muxr layout metadata to load before resize"))?;
            let before_regions = self::layout_active_tab_pane_regions(&before_resize, &size)?;
            pretty_assertions::assert_eq!(
                before_regions.iter().map(|(id, ..)| id.as_str()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('R'),
                    modifiers: ClientKeyModifiers::SHIFT_ALT,
                    raw_bytes: b"\x1bR".to_vec(),
                }))
                .await?;
            drop(self::read_until_layout(&mut client).await?);
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('h'),
                    modifiers: ClientKeyModifiers::NONE,
                    raw_bytes: b"h".to_vec(),
                }))
                .await?;

            let layout = self::read_until_layout(&mut client).await?;
            let tab = layout
                .tabs()
                .first()
                .ok_or_else(|| report!("expected tab after resize"))?;
            pretty_assertions::assert_eq!(tab.active_pane().to_string(), "pane-2");
            pretty_assertions::assert_eq!(
                tab.panes().iter().map(|pane| pane.id.to_string()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );
            let persisted = crate::state::persisted::load_metadata(&paths, &session)?
                .ok_or_else(|| report!("expected muxr layout metadata to load"))?;
            let after_regions = self::layout_active_tab_pane_regions(&persisted, &size)?;
            pretty_assertions::assert_eq!(
                after_regions.iter().map(|(id, ..)| id.as_str()).collect::<Vec<_>>(),
                vec!["pane-1", "pane-2"],
            );
            let before_first = &before_regions[0];
            let before_second = &before_regions[1];
            let after_first = &after_regions[0];
            let after_second = &after_regions[1];
            pretty_assertions::assert_eq!(
                (after_first.1, after_first.2, after_first.4),
                (before_first.1, before_first.2, before_first.4),
            );
            pretty_assertions::assert_eq!((after_second.2, after_second.4), (before_second.2, before_second.4));
            assert2::assert!(after_first.3 < before_first.3);
            assert2::assert!(after_second.3 > before_second.3);
            pretty_assertions::assert_eq!(after_second.1, after_first.1 + after_first.3 + 1);
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Esc,
                    modifiers: ClientKeyModifiers::NONE,
                    raw_bytes: b"\x1b".to_vec(),
                }))
                .await?;
            client
                .writer
                .send_request(&ClientRequest::Key(ClientKey {
                    code: ClientKeyCode::Char('x'),
                    modifiers: ClientKeyModifiers::NONE,
                    raw_bytes: b"x\n".to_vec(),
                }))
                .await?;
            self::read_until_render_contains(&mut client, b"x").await?;
            self::detach_client(client).await?;
            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_shell_outputs_while_detached_replays_output_on_reattach() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(2),
                self::shell_cmd("/bin/sh")
                    .arg("-c")
                    .arg("printf first; sleep 1; printf second; sleep 30"),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut first_client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut first_client, b"first").await?;
            self::detach_client(first_client).await?;

            tokio::time::sleep(Duration::from_millis(1200)).await;

            let mut second_client = self::open_attached_client(&session, &paths).await?;
            self::read_until_render_contains(&mut second_client, b"second").await?;
            self::detach_client(second_client).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_client_floods_input_still_sends_output() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd("/bin/sh")
                    .arg("-c")
                    .arg("sleep 0.1; printf ready; sleep 30"),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::attach_request(&session)?).await?;
            let Some(ServerEvent::Attached(_)) = connection.recv_event().await? else {
                return Err(report!("expected server attached response"));
            };
            let (mut reader, mut writer) = connection.split();
            let flood_handle = tokio::spawn(async move {
                loop {
                    if writer.send_request(&ClientRequest::Input(Vec::new())).await.is_err() {
                        break;
                    }
                }
            });

            let read_result = self::read_reader_until_render_contains(&mut reader, b"ready").await;
            drop(reader);
            flood_handle.abort();
            drop(flood_handle.await);
            let join_result = self::join_server_with_timeout(handle);

            read_result?;
            join_result
        })
    }

    #[test]
    fn test_serve_when_shell_floods_output_still_detaches() -> rootcause::Result<()> {
        self::runtime()?.block_on(async {
            let tempdir = tempfile::tempdir()?;
            let (session, paths) = self::session_paths(tempdir.path(), "work")?;
            let handle = self::spawn_test_server_with_shell(
                &session,
                &paths,
                Some(1),
                self::shell_cmd("/bin/sh").arg("-c").arg("while :; do printf x; done"),
            );

            self::wait_for_socket(&paths.socket)?;
            let mut connection = self::connect_client(&paths).await?;
            connection.send_request(&self::attach_request(&session)?).await?;
            let Some(ServerEvent::Attached(_)) = connection.recv_event().await? else {
                return Err(report!("expected server attached response"));
            };
            self::read_connection_until_render_contains(&mut connection, b"x").await?;
            connection.send_request(&ClientRequest::Detach).await?;
            self::read_connection_until_detached(&mut connection).await?;

            self::join_server(handle)
        })
    }

    #[test]
    fn test_serve_when_shell_exits_removes_socket_and_pid() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(
            &session,
            &paths,
            None,
            self::shell_cmd("/bin/sh").arg("-c").arg("printf done"),
        );

        self::wait_for_socket(&paths.socket)?;
        self::join_server_with_timeout(handle)?;

        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        self::assert_final_layout_metadata(&paths, 0, true)?;
        Ok(())
    }

    #[test]
    fn test_serve_when_shell_exits_with_error_persists_exit_status() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;
        let handle = self::spawn_test_server_with_shell(
            &session,
            &paths,
            None,
            self::shell_cmd("/bin/sh").arg("-c").arg("exit 7"),
        );

        self::wait_for_socket(&paths.socket)?;
        self::join_server_with_timeout(handle)?;

        self::assert_final_layout_metadata(&paths, 7, false)?;
        Ok(())
    }

    #[test]
    fn test_serve_when_startup_fails_after_bind_removes_socket_and_pid() -> rootcause::Result<()> {
        let tempdir = tempfile::tempdir()?;
        let (session, paths) = self::session_paths(tempdir.path(), "work")?;

        let result = serve(&ServerConfig {
            session,
            paths: paths.clone(),
            max_accepted_connections: None,
            shell_cmd: self::shell_cmd("/bin/muxr-missing-shell"),
        });

        assert2::assert!(result.is_err());
        assert2::assert!(!paths.socket.exists());
        assert2::assert!(!paths.pid.exists());
        Ok(())
    }

    fn spawn_test_server(
        session: &SessionName,
        paths: &SessionPaths,
        max_accepted_connections: usize,
    ) -> thread::JoinHandle<rootcause::Result<()>> {
        self::spawn_test_server_with_shell(
            session,
            paths,
            Some(max_accepted_connections),
            self::shell_cmd("/bin/sh").arg("-c").arg("sleep 30"),
        )
    }

    fn spawn_test_server_with_shell(
        session: &SessionName,
        paths: &SessionPaths,
        max_accepted_connections: Option<usize>,
        shell_cmd: ShellCmd,
    ) -> thread::JoinHandle<rootcause::Result<()>> {
        thread::spawn({
            let session = session.clone();
            let paths = paths.clone();
            move || {
                serve(&ServerConfig {
                    session,
                    paths,
                    max_accepted_connections,
                    shell_cmd,
                })
            }
        })
    }

    async fn connect_client(paths: &SessionPaths) -> rootcause::Result<ClientConnection> {
        let started_at = Instant::now();

        loop {
            match ClientConnection::connect(&paths.socket).await {
                Ok(connection) => return Ok(connection),
                Err(error) => {
                    if started_at.elapsed() > SERVER_READY_TIMEOUT {
                        return Err(error);
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn open_attached_client(
        session: &SessionName,
        paths: &SessionPaths,
    ) -> rootcause::Result<AttachedTestClient> {
        let mut connection = self::connect_client(paths).await?;

        connection.send_request(&self::attach_request(session)?).await?;
        let event = connection.recv_event().await?;
        let Some(ServerEvent::Attached(attached)) = event else {
            return Err(report!("expected server attached response").attach(format!("{event:?}")));
        };
        let layout = attached.layout;
        let pane_regions = attached.pane_regions;
        let (reader, writer) = connection.split();

        Ok(AttachedTestClient {
            layout,
            pane_regions,
            reader,
            writer,
        })
    }

    async fn attach_and_detach(session: &SessionName, paths: &SessionPaths) -> rootcause::Result<()> {
        let client = self::open_attached_client(session, paths).await?;

        self::detach_client(client).await?;
        Ok(())
    }

    async fn detach_client(mut client: AttachedTestClient) -> rootcause::Result<()> {
        client.writer.send_request(&ClientRequest::Detach).await?;
        self::read_client_until_detached(&mut client).await
    }

    async fn read_client_until_detached(client: &mut AttachedTestClient) -> rootcause::Result<()> {
        loop {
            match client.reader.recv_event().await? {
                Some(ServerEvent::Detached) => break,
                Some(ServerEvent::Ping) => client.writer.send_request(&ClientRequest::Pong).await?,
                Some(
                    ServerEvent::Attached(_)
                    | ServerEvent::Pong
                    | ServerEvent::Layout(_)
                    | ServerEvent::SidebarLayout(_)
                    | ServerEvent::PaneRegions(_)
                    | ServerEvent::Render(_),
                ) => {}
                Some(event) => return Err(report!("expected detached event").attach(format!("{event:?}"))),
                None => return Err(report!("expected detached event")),
            }
        }
        Ok(())
    }

    fn join_server(handle: thread::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
        handle
            .join()
            .unwrap_or_else(|_| Err(report!("test muxr server thread panicked")))
    }

    fn join_server_with_timeout(handle: thread::JoinHandle<rootcause::Result<()>>) -> rootcause::Result<()> {
        let started_at = Instant::now();
        while !handle.is_finished() {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr test server exit"));
            }

            thread::sleep(Duration::from_millis(10));
        }

        self::join_server(handle)
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

    fn shell_cmd(program: &str) -> ShellCmd {
        ShellCmd::new(program).expect("test shell cmd must have a nonempty program path")
    }

    fn server_config(base: &Path, raw: &str) -> rootcause::Result<ServerConfig> {
        let (session, paths) = self::session_paths(base, raw)?;
        Ok(ServerConfig {
            session,
            paths,
            max_accepted_connections: None,
            shell_cmd: self::shell_cmd("/bin/sh"),
        })
    }

    fn metadata(cmd_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            cmd_label: cmd_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }

    fn force_balanced_test_split_ratio(layout: &mut SessionLayout) -> rootcause::Result<()> {
        let ratio = PaneSplitRatio::new(BALANCED_TEST_SPLIT_RATIO)?;
        for tab in &mut layout.entries {
            self::force_balanced_pane_tree_split_ratio(&mut tab.pane_tree, ratio);
        }
        Ok(())
    }

    fn force_balanced_pane_tree_split_ratio(pane_tree: &mut PaneTree, ratio: PaneSplitRatio) {
        match pane_tree {
            PaneTree::Pane(_) => {}
            PaneTree::Split {
                first_ratio,
                first,
                second,
                ..
            } => {
                *first_ratio = ratio;
                self::force_balanced_pane_tree_split_ratio(first, ratio);
                self::force_balanced_pane_tree_split_ratio(second, ratio);
            }
        }
    }

    fn layout_tab_ids(layout: &SessionLayout) -> rootcause::Result<Vec<String>> {
        Ok(layout
            .snapshot()?
            .tabs()
            .iter()
            .map(|tab| tab.id().to_string())
            .collect::<Vec<_>>())
    }

    fn layout_active_tab_pane_ids(layout: &SessionLayout) -> rootcause::Result<Vec<String>> {
        let snapshot = layout.snapshot()?;
        let active_tab = snapshot
            .tabs()
            .iter()
            .find(|tab| tab.id() == snapshot.active_tab())
            .ok_or_else(|| report!("expected active tab in muxr test layout snapshot"))?;

        Ok(active_tab.panes().iter().map(|pane| pane.id.to_string()).collect())
    }

    fn layout_active_tab_pane_regions(
        layout: &SessionLayout,
        size: &TerminalSize,
    ) -> rootcause::Result<Vec<PaneRegionTuple>> {
        Ok(layout
            .pane_regions(size)?
            .iter()
            .map(|region| {
                (
                    region.id.to_string(),
                    region.area.origin.col,
                    region.area.origin.row,
                    region.area.size.cols,
                    region.area.size.rows,
                )
            })
            .collect())
    }

    fn layout_active_tab_pane_borders(
        layout: &SessionLayout,
        size: &TerminalSize,
    ) -> rootcause::Result<Vec<(PaneBorderAxis, u16, u16, u16)>> {
        Ok(layout
            .pane_layout(size)?
            .borders()
            .iter()
            .map(|border| (border.axis(), border.col(), border.row(), border.len()))
            .collect())
    }

    fn wait_for_runtime_snapshot_contains(
        runtimes: &Mutex<PaneRuntimes>,
        pane_id: PaneId,
        needle: &str,
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            let rendered = {
                let runtimes = self::lock_mutex(runtimes, "pane runtimes")?;
                runtimes
                    .handle(pane_id)?
                    .render_snapshot()?
                    .rows()
                    .iter()
                    .flat_map(|row| row.cells().iter().map(RenderCell::text))
                    .collect::<String>()
            };
            if rendered.contains(needle) {
                return Ok(());
            }
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr runtime snapshot").attach(rendered));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn make_public_session_dirs(paths: &SessionPaths) -> rootcause::Result<()> {
        for path in self::session_private_dirs(paths)? {
            fs::create_dir_all(path).context("failed to create public muxr test dir")?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))
                .context("failed to set public muxr test dir permissions")?;
        }
        Ok(())
    }

    fn assert_session_paths_are_private(paths: &SessionPaths) -> rootcause::Result<()> {
        for path in self::session_private_dirs(paths)? {
            self::assert_mode(path, PRIVATE_DIR_MODE)?;
        }
        self::assert_mode(&paths.socket, PRIVATE_SOCKET_MODE)?;
        Ok(())
    }

    fn session_private_dirs(paths: &SessionPaths) -> rootcause::Result<Vec<&Path>> {
        let socket_root = self::parent_path(&paths.socket, "socket root")?;
        let state_root = self::parent_path(socket_root, "state root")?;
        let sessions_root = self::parent_path(&paths.root, "sessions root")?;

        Ok(vec![
            state_root,
            sessions_root,
            socket_root,
            paths.root.as_path(),
            paths.panes.as_path(),
        ])
    }

    fn parent_path<'a>(path: &'a Path, label: &str) -> rootcause::Result<&'a Path> {
        path.parent()
            .ok_or_else(|| report!("muxr test path has no parent").attach(format!("label={label}")))
    }

    fn assert_mode(path: &Path, expected_mode: u32) -> rootcause::Result<()> {
        let mode = fs::metadata(path)
            .context("failed to inspect muxr test path mode")?
            .permissions()
            .mode()
            & 0o777;

        pretty_assertions::assert_eq!(mode, expected_mode);
        Ok(())
    }

    fn wait_for_socket(path: &Path) -> rootcause::Result<()> {
        self::wait_for_path(path)
    }

    fn wait_for_path(path: &Path) -> rootcause::Result<()> {
        let started_at = Instant::now();
        loop {
            if path.exists() {
                return Ok(());
            }

            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr test path").attach(path.display().to_string()));
            }

            thread::sleep(Duration::from_millis(10));
        }
    }

    fn attach_request(session: &SessionName) -> rootcause::Result<ClientRequest> {
        Ok(ClientRequest::Attach(AttachRequest {
            session: session.clone(),
            terminal_size: self::terminal_size()?,
        }))
    }

    fn terminal_size() -> rootcause::Result<TerminalSize> {
        TerminalSize::new(80, 24)
    }

    async fn read_until_layout(client: &mut AttachedTestClient) -> rootcause::Result<LayoutSnapshot> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr layout update"));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Layout(layout) => {
                    client.layout = layout.clone();
                    return Ok(layout);
                }
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::Render(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_pane_regions_matching(
        client: &mut AttachedTestClient,
        description: &str,
        condition: impl Fn(&PaneRegionsSnapshot) -> rootcause::Result<bool>,
    ) -> rootcause::Result<PaneRegionsSnapshot> {
        if condition(&client.pane_regions)? {
            return Ok(client.pane_regions.clone());
        }
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr pane regions update").attach(description.to_owned()));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::PaneRegions(regions) => {
                    client.pane_regions = regions.clone();
                    if condition(&regions)? {
                        return Ok(regions);
                    }
                }
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::Render(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_scroll_pane_line_result(
        client: &mut AttachedTestClient,
    ) -> rootcause::Result<(ClientMousePosition, PaneScrollDirection, bool)> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr scroll line result"));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::ScrollPaneLineResult {
                    position,
                    direction,
                    scrolled,
                } => return Ok((position, direction, scrolled)),
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::Render(_)
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_sidebar_layout(client: &mut AttachedTestClient) -> rootcause::Result<LayoutSnapshot> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr sidebar layout update"));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::SidebarLayout(layout) => {
                    client.layout = layout.clone();
                    return Ok(layout);
                }
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::Render(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_render_contains(client: &mut AttachedTestClient, needle: &[u8]) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut rendered = String::new();
        let needle = String::from_utf8_lossy(needle);

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr rendered pty output").attach(rendered));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Render(update) => {
                    rendered.push_str(&self::render_update_text(&update));
                    if rendered.contains(needle.as_ref()) {
                        return Ok(());
                    }
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_until_render_contains_hex_bytes(
        client: &mut AttachedTestClient,
        expected: &[&str],
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut rendered = String::new();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr rendered hex bytes").attach(rendered));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), client.reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Layout(layout) => client.layout = layout,
                ServerEvent::PaneRegions(regions) => client.pane_regions = regions,
                ServerEvent::Render(update) => {
                    rendered.push_str(&self::render_update_text(&update));
                    let tokens = rendered.split_whitespace().collect::<Vec<_>>();
                    if tokens.windows(expected.len()).any(|window| window == expected) {
                        return Ok(());
                    }
                }
                ServerEvent::Ping => client.writer.send_request(&ClientRequest::Pong).await?,
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Pong
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_reader_until_render_contains(reader: &mut ClientEventReader, needle: &[u8]) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut rendered = String::new();
        let needle = String::from_utf8_lossy(needle);

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr rendered pty output").attach(rendered));
            }

            let event = match tokio::time::timeout(Duration::from_millis(50), reader.recv_event()).await {
                Ok(Ok(Some(event))) => event,
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => continue,
            };

            match event {
                ServerEvent::Render(update) => {
                    rendered.push_str(&self::render_update_text(&update));
                    if rendered.contains(needle.as_ref()) {
                        return Ok(());
                    }
                }
                ServerEvent::Error(error) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                ServerEvent::Attached(_)
                | ServerEvent::Deleted
                | ServerEvent::Ping
                | ServerEvent::Pong
                | ServerEvent::Layout(_)
                | ServerEvent::SidebarLayout(_)
                | ServerEvent::PaneRegions(_)
                | ServerEvent::ScrollPaneLineResult { .. }
                | ServerEvent::Detached => {}
            }
        }
    }

    async fn read_connection_until_render_contains(
        connection: &mut ClientConnection,
        needle: &[u8],
    ) -> rootcause::Result<()> {
        let started_at = Instant::now();
        let mut rendered = String::new();
        let needle = String::from_utf8_lossy(needle);

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr rendered pty output").attach(rendered));
            }

            match tokio::time::timeout(Duration::from_millis(50), connection.recv_event()).await {
                Ok(Ok(Some(ServerEvent::Render(update)))) => {
                    rendered.push_str(&self::render_update_text(&update));
                    if rendered.contains(needle.as_ref()) {
                        return Ok(());
                    }
                }
                Ok(Ok(Some(ServerEvent::Ping))) => connection.send_request(&ClientRequest::Pong).await?,
                Ok(Ok(Some(ServerEvent::Error(error)))) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                Ok(Ok(
                    Some(
                        ServerEvent::Attached(_)
                        | ServerEvent::Deleted
                        | ServerEvent::Pong
                        | ServerEvent::Layout(_)
                        | ServerEvent::SidebarLayout(_)
                        | ServerEvent::PaneRegions(_)
                        | ServerEvent::ScrollPaneLineResult { .. }
                        | ServerEvent::Detached,
                    )
                    | None,
                ))
                | Err(_) => {}
                Ok(Err(error)) => return Err(error),
            }
        }
    }

    async fn read_connection_until_detached(connection: &mut ClientConnection) -> rootcause::Result<()> {
        let started_at = Instant::now();

        loop {
            if started_at.elapsed() > SERVER_READY_TIMEOUT {
                return Err(report!("timed out waiting for muxr detach ack"));
            }

            match tokio::time::timeout(Duration::from_millis(50), connection.recv_event()).await {
                Ok(Ok(Some(ServerEvent::Detached))) => return Ok(()),
                Ok(Ok(Some(ServerEvent::Ping))) => connection.send_request(&ClientRequest::Pong).await?,
                Ok(Ok(Some(ServerEvent::Error(error)))) => {
                    return Err(report!("muxr test server returned error").attach(format!("{error:?}")));
                }
                Ok(Ok(
                    Some(
                        ServerEvent::Attached(_)
                        | ServerEvent::Deleted
                        | ServerEvent::Pong
                        | ServerEvent::Layout(_)
                        | ServerEvent::SidebarLayout(_)
                        | ServerEvent::PaneRegions(_)
                        | ServerEvent::Render(_)
                        | ServerEvent::ScrollPaneLineResult { .. },
                    )
                    | None,
                ))
                | Err(_) => {}
                Ok(Err(error)) => return Err(error),
            }
        }
    }

    fn render_update_text(update: &RenderUpdate) -> String {
        match update {
            RenderUpdate::Baseline(baseline) => self::render_rows_text(baseline.rows()),
            RenderUpdate::Diff(diff) => self::render_rows_text(diff.rows()),
        }
    }

    fn render_rows_text(rows: &[RenderRowSpan]) -> String {
        rows.iter()
            .map(|row| row.cells().iter().map(RenderCell::text).collect::<String>())
            .collect()
    }

    fn pane_region<'a>(regions: &'a PaneRegionsSnapshot, pane_id: &str) -> rootcause::Result<&'a PaneRegionSnapshot> {
        regions
            .regions()
            .iter()
            .find(|region| region.id().to_string() == pane_id)
            .ok_or_else(|| report!("expected muxr test pane region").attach(format!("pane_id={pane_id}")))
    }

    fn pane_position(regions: &PaneRegionsSnapshot, pane_id: &str) -> rootcause::Result<ClientMousePosition> {
        let region = self::pane_region(regions, pane_id)?;
        Ok(ClientMousePosition {
            row: region.row(),
            col: region.col(),
        })
    }

    fn assert_final_layout_metadata(
        paths: &SessionPaths,
        expected_code: u64,
        expected_success: bool,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let pane = &layout["tabs"][0]["pane_tree"];

        pretty_assertions::assert_eq!(layout["version"].as_u64(), Some(u64::from(crate::state::VERSION)));
        pretty_assertions::assert_eq!(layout["session"].as_str(), Some("work"));
        pretty_assertions::assert_eq!(layout["active_tab"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(pane["id"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(pane["cmd_label"].as_str(), Some("sh"));
        assert2::assert!(pane["started_at"].as_u64().is_some());
        pretty_assertions::assert_eq!(pane["state"]["kind"].as_str(), Some("process_exited"));
        assert2::assert!(pane["state"]["at"].as_u64().is_some());
        pretty_assertions::assert_eq!(pane["state"]["status"]["code"].as_u64(), Some(expected_code));
        pretty_assertions::assert_eq!(pane["state"]["status"]["success"].as_bool(), Some(expected_success));
        Ok(())
    }

    fn assert_layout_metadata_tabs(
        paths: &SessionPaths,
        expected_tabs: &[u64],
        expected_active: u64,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let Some(tabs) = layout["tabs"].as_array() else {
            return Err(report!("muxr test layout metadata tabs are missing"));
        };
        let actual_tabs = tabs
            .iter()
            .map(|tab| {
                tab["id"]
                    .as_u64()
                    .ok_or_else(|| report!("muxr test layout metadata tab id is missing"))
            })
            .collect::<rootcause::Result<Vec<_>>>()?;

        pretty_assertions::assert_eq!(layout["active_tab"].as_u64(), Some(expected_active));
        pretty_assertions::assert_eq!(actual_tabs, expected_tabs.to_vec());
        Ok(())
    }

    fn assert_layout_metadata_panes(
        paths: &SessionPaths,
        expected_panes: &[u64],
        expected_active: u64,
    ) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let actual_panes = self::json_pane_tree_pane_ids(&layout["tabs"][0]["pane_tree"])?;

        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_u64(), Some(expected_active));
        pretty_assertions::assert_eq!(actual_panes, expected_panes.to_vec());
        Ok(())
    }

    fn assert_final_closed_layout_metadata(paths: &SessionPaths) -> rootcause::Result<()> {
        let layout: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.layout).context("failed to read muxr test layout metadata")?)
                .context("failed to parse muxr test layout metadata")?;
        let pane = &layout["tabs"][0]["pane_tree"];

        pretty_assertions::assert_eq!(layout["active_tab"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(layout["tabs"][0]["active_pane"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(pane["id"].as_u64(), Some(1));
        pretty_assertions::assert_eq!(pane["state"]["kind"].as_str(), Some("closed"));
        assert2::assert!(pane["state"]["at"].as_u64().is_some());
        assert2::assert!(pane["state"].get("status").is_none());
        Ok(())
    }

    fn json_pane_tree_pane_ids(node: &serde_json::Value) -> rootcause::Result<Vec<u64>> {
        let mut ids = Vec::new();
        self::collect_json_pane_tree_pane_ids(node, &mut ids)?;
        Ok(ids)
    }

    fn collect_json_pane_tree_pane_ids(node: &serde_json::Value, ids: &mut Vec<u64>) -> rootcause::Result<()> {
        match node["kind"].as_str() {
            Some("pane") => {
                let Some(id) = node["id"].as_u64() else {
                    return Err(report!("muxr test layout metadata pane id is missing"));
                };
                ids.push(id);
                Ok(())
            }
            Some("split") => {
                self::collect_json_pane_tree_pane_ids(&node["first"], ids)?;
                self::collect_json_pane_tree_pane_ids(&node["second"], ids)
            }
            Some(kind) => {
                Err(report!("muxr test layout metadata pane tree kind is invalid").attach(format!("kind={kind}")))
            }
            None => Err(report!("muxr test layout metadata pane tree kind is missing")),
        }
    }

    fn runtime() -> rootcause::Result<tokio::runtime::Runtime> {
        Ok(tokio::runtime::Runtime::new().context("failed to build muxr server test runtime")?)
    }
}
