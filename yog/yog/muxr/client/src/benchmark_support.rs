//! Feature-gated performance workloads and behavioral oracle for muxr development.

use muxr_config::MuxrConfig;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneId;
use muxr_core::PaneMouseMode;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneRegionsSnapshot;
use muxr_core::PaneSnapshot;
use muxr_core::RenderBaseline;
use muxr_core::RenderCell;
use muxr_core::RenderCursor;
use muxr_core::RenderCursorShape;
use muxr_core::RenderCursorVisibility;
use muxr_core::RenderDiff;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderUpdate;
use muxr_core::ServerEvent;
use muxr_core::TabId;
use muxr_core::TabSnapshot;
use muxr_core::TerminalSize;
use muxr_core::TrackedProcessState;
use muxr_core::decode_server_event;
use muxr_core::encode_server_event;
use rootcause::report;

use crate::frame_buffer::ApplyOutcome;
use crate::frame_buffer::FrameBuffer;

/// Stable metadata recorded beside each benchmark sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkloadCounters {
    pub cells_snapshotted: u64,
    pub panes_snapshotted: u64,
    pub payload_copies: u64,
}

/// One deterministic representative muxr workload.
#[derive(Clone, Debug)]
pub struct Workload {
    counters: WorkloadCounters,
    events: Vec<ServerEvent>,
    name: &'static str,
}

/// Observable result used by both Criterion and the differential oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkloadResult {
    pub counters: WorkloadCounters,
    pub encoded_bytes: u64,
    pub terminal_bytes: Vec<u8>,
}

impl Workload {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    #[must_use]
    pub const fn counters(&self) -> WorkloadCounters {
        self.counters
    }

    /// Execute protocol serialization, validation, framebuffer application, and terminal rendering.
    ///
    /// # Errors
    /// Returns an error if a fixture or any exercised production boundary rejects the workload.
    pub fn run(&self) -> rootcause::Result<WorkloadResult> {
        let mut encoded_bytes = 0_u64;
        let mut frame_buffer = FrameBuffer::default();
        let mut terminal_bytes = Vec::new();

        for event in &self.events {
            let frame = encode_server_event(event)?;
            encoded_bytes = encoded_bytes
                .checked_add(u64::try_from(frame.as_bytes().len())?)
                .ok_or_else(|| report!("muxr benchmark encoded byte count overflowed"))?;
            let decoded = decode_server_event(frame.as_bytes())?;
            if decoded != *event {
                return Err(
                    report!("muxr benchmark protocol oracle mismatch").attach(format!("workload={}", self.name))
                );
            }
            let ServerEvent::Render(update) = decoded else {
                continue;
            };
            let ApplyOutcome::Applied(changes) = frame_buffer.apply(update)? else {
                return Err(
                    report!("muxr benchmark unexpectedly requested render resynchronization")
                        .attach(format!("workload={}", self.name)),
                );
            };
            frame_buffer.queue_at_with_selection(
                &mut terminal_bytes,
                &changes,
                0,
                0,
                None,
                MuxrConfig::default().selection.bg,
            )?;
        }

        Ok(WorkloadResult {
            counters: self.counters,
            encoded_bytes,
            terminal_bytes,
        })
    }
}

/// Build the five S1 workloads in stable reporting order.
///
/// # Errors
/// Returns an error if a deterministic fixture violates a muxr domain invariant.
pub fn workloads() -> rootcause::Result<Vec<Workload>> {
    Ok(vec![
        two_pane_interactive()?,
        four_pane_output_burst()?,
        one_dirty_pane()?,
        url_heavy_frame()?,
        seven_tab_sidebar()?,
    ])
}

/// Run every workload twice and require byte-for-byte deterministic observables.
///
/// # Errors
/// Returns the first production-boundary or determinism mismatch.
pub fn verify_oracle() -> rootcause::Result<()> {
    for workload in self::workloads()? {
        let first = workload.run()?;
        let second = workload.run()?;
        if first != second {
            return Err(
                report!("muxr benchmark determinism oracle mismatch").attach(format!("workload={}", workload.name))
            );
        }
    }
    self::verify_edge_case_trace()
}

fn two_pane_interactive() -> rootcause::Result<Workload> {
    let size = TerminalSize::new(120, 40)?;
    let baseline = render_baseline(1, &size, None)?;
    let diff = RenderDiff::new(1, 2, size, visible_cursor(3, 7), vec![text_row(7, 0, 120, None)?])?;
    Ok(render_workload(
        "two_pane_interactive",
        2,
        4_800,
        vec![RenderUpdate::Baseline(baseline), RenderUpdate::Diff(diff)],
    ))
}

fn four_pane_output_burst() -> rootcause::Result<Workload> {
    let size = TerminalSize::new(160, 60)?;
    Ok(render_workload(
        "four_pane_8k_output_burst",
        4,
        9_600,
        vec![RenderUpdate::Baseline(render_baseline(1, &size, None)?)],
    ))
}

fn one_dirty_pane() -> rootcause::Result<Workload> {
    let size = TerminalSize::new(160, 60)?;
    let baseline = render_baseline(1, &size, None)?;
    let rows = (0..30)
        .map(|row| text_row(row, 0, 80, None))
        .collect::<rootcause::Result<Vec<_>>>()?;
    let diff = RenderDiff::new(1, 2, size, visible_cursor(0, 0), rows)?;
    Ok(render_workload(
        "one_dirty_pane_frame",
        4,
        9_600,
        vec![RenderUpdate::Baseline(baseline), RenderUpdate::Diff(diff)],
    ))
}

fn url_heavy_frame() -> rootcause::Result<Workload> {
    let size = TerminalSize::new(320, 90)?;
    Ok(render_workload(
        "url_heavy_320x90_frame",
        4,
        28_800,
        vec![RenderUpdate::Baseline(render_baseline(
            1,
            &size,
            Some("https://example.com/muxr/performance/reference"),
        )?)],
    ))
}

fn seven_tab_sidebar() -> rootcause::Result<Workload> {
    let mut tabs = Vec::new();
    for id in 1..=7 {
        let pane_id = PaneId::new(id)?;
        tabs.push(TabSnapshot::new(
            TabId::new(id)?,
            format!("tab-{id}"),
            pane_id,
            vec![PaneSnapshot {
                tracked_process_state: TrackedProcessState::Busy,
                cwd: format!("/tmp/worktree-{id}"),
                cmd_label: Some("codex".to_owned()),
                focus_seq: u64::from(id),
                id: pane_id,
                title: format!("agent-{id}"),
            }],
        )?);
    }
    let layout = LayoutSnapshot::new(TabId::new(1)?, tabs)?;
    Ok(Workload {
        counters: WorkloadCounters {
            cells_snapshotted: 0,
            panes_snapshotted: 7,
            payload_copies: 1,
        },
        events: vec![ServerEvent::SidebarLayout(layout)],
        name: "seven_tab_sidebar_update",
    })
}

fn render_workload(name: &'static str, panes: u64, cells: u64, updates: Vec<RenderUpdate>) -> Workload {
    Workload {
        counters: WorkloadCounters {
            cells_snapshotted: cells,
            panes_snapshotted: panes,
            // ProtocolFrame currently copies rkyv's serialization buffer once while prepending frame magic.
            payload_copies: u64::try_from(updates.len()).unwrap_or(u64::MAX),
        },
        events: updates.into_iter().map(ServerEvent::Render).collect(),
        name,
    }
}

fn render_baseline(seq: u64, size: &TerminalSize, hyperlink: Option<&str>) -> rootcause::Result<RenderBaseline> {
    let rows = (0..size.rows())
        .map(|row| text_row(row, 0, size.cols(), hyperlink))
        .collect::<rootcause::Result<Vec<_>>>()?;
    RenderBaseline::new(seq, size.clone(), visible_cursor(0, 0), rows)
}

fn text_row(row: u16, col: u16, width: u16, hyperlink: Option<&str>) -> rootcause::Result<RenderRowSpan> {
    let mut cells = Vec::with_capacity(usize::from(width));
    for index in 0..width {
        let cell = if index == 0 && row == 0 && width >= 2 {
            RenderCell::wide("表", RenderStyle::default())
        } else if index == 1 && row == 0 && width >= 2 {
            RenderCell::wide_continuation(RenderStyle::default())
        } else {
            RenderCell::narrow(
                char::from(b'a'.saturating_add(u8::try_from(index % 26)?)).to_string(),
                RenderStyle::default(),
            )
        };
        cells.push(match hyperlink {
            Some(uri) => cell.with_hyperlink_uri(uri)?,
            None => cell,
        });
    }
    RenderRowSpan::new(row, col, cells)
}

const fn visible_cursor(row: u16, col: u16) -> RenderCursor {
    RenderCursor {
        col,
        row,
        shape: RenderCursorShape::Default,
        visibility: RenderCursorVisibility::Visible,
    }
}

fn verify_resynchronization_oracle() -> rootcause::Result<()> {
    let size = TerminalSize::new(2, 1)?;
    let mut frame_buffer = FrameBuffer::default();
    let stale = RenderDiff::new(7, 8, size, visible_cursor(0, 0), vec![text_row(0, 0, 2, None)?])?;
    if frame_buffer.apply(RenderUpdate::Diff(stale))? != ApplyOutcome::NeedsResync {
        return Err(report!("muxr benchmark resynchronization oracle mismatch"));
    }
    Ok(())
}

fn verify_edge_case_trace() -> rootcause::Result<()> {
    let size = TerminalSize::new(8, 2)?;
    let pane_id = PaneId::new(1)?;
    let region_before = PaneRegionSnapshot::new(pane_id, 0, 0, 8, 2, PaneMouseMode::None, 40)?;
    let region_after = PaneRegionSnapshot::new(pane_id, 0, 0, 8, 2, PaneMouseMode::None, 41)?;
    let trace = vec![
        // Scrollback changes the stable visible row even when the rendered pixels happen to match.
        ServerEvent::PaneRegions(PaneRegionsSnapshot::new(vec![region_before])?),
        ServerEvent::Render(RenderUpdate::Baseline(render_baseline(1, &size, None)?)),
        // These full-row changes model focus, attention, and fullscreen recomposition at the protocol boundary.
        ServerEvent::Render(RenderUpdate::Diff(RenderDiff::new(
            1,
            2,
            size.clone(),
            visible_cursor(0, 0),
            vec![text_row(0, 0, 8, None)?],
        )?)),
        // A cursor-only update must advance sequence without inventing dirty rows.
        ServerEvent::Render(RenderUpdate::Diff(RenderDiff::new(
            2,
            3,
            size,
            visible_cursor(1, 3),
            Vec::new(),
        )?)),
        ServerEvent::PaneRegions(PaneRegionsSnapshot::new(vec![region_after])?),
        // Resize requires a new baseline with the new framebuffer dimensions.
        ServerEvent::Render(RenderUpdate::Baseline(render_baseline(
            4,
            &TerminalSize::new(10, 3)?,
            None,
        )?)),
    ];
    let workload = Workload {
        counters: WorkloadCounters {
            cells_snapshotted: 16,
            panes_snapshotted: 1,
            payload_copies: u64::try_from(trace.len())?,
        },
        events: trace,
        name: "edge_case_trace",
    };
    if workload.run()? != workload.run()? {
        return Err(report!("muxr benchmark edge-case trace mismatch"));
    }
    self::verify_resynchronization_oracle()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_workloads_when_repeated_match_behavioral_oracle() -> rootcause::Result<()> {
        verify_oracle()
    }
}
