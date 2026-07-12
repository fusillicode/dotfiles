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
use muxr_core::RenderColor;
use muxr_core::RenderCursor;
use muxr_core::RenderCursorShape;
use muxr_core::RenderCursorVisibility;
use muxr_core::RenderDiff;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
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

/// Operations derived from one client benchmark's event stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkloadCounters {
    pub dirty_cells_encoded: u64,
    pub frames_encoded: u64,
    pub render_cells_encoded: u64,
}

/// One deterministic representative muxr workload.
#[derive(Clone, Debug)]
pub struct Workload {
    counters: WorkloadCounters,
    events: Vec<ServerEvent>,
    expected_terminal: TerminalFingerprint,
    name: &'static str,
}

/// Observable result used by both Criterion and the behavioral oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkloadResult {
    pub counters: WorkloadCounters,
    pub encoded_bytes: u64,
    pub terminal_bytes: Vec<u8>,
}

impl Workload {
    fn new(
        name: &'static str,
        events: Vec<ServerEvent>,
        expected_terminal: TerminalFingerprint,
    ) -> rootcause::Result<Self> {
        Ok(Self {
            counters: Self::operation_counts(&events)?,
            events,
            expected_terminal,
            name,
        })
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
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

    fn operation_counts(events: &[ServerEvent]) -> rootcause::Result<WorkloadCounters> {
        let mut counters = WorkloadCounters {
            dirty_cells_encoded: 0,
            frames_encoded: u64::try_from(events.len())?,
            render_cells_encoded: 0,
        };
        for event in events {
            let ServerEvent::Render(update) = event else {
                continue;
            };
            let cells = Self::render_update_cell_count(update)?;
            counters.render_cells_encoded = counters
                .render_cells_encoded
                .checked_add(cells)
                .ok_or_else(|| report!("muxr benchmark render cell count overflowed"))?;
            if matches!(update, RenderUpdate::Diff(_)) {
                counters.dirty_cells_encoded = counters
                    .dirty_cells_encoded
                    .checked_add(cells)
                    .ok_or_else(|| report!("muxr benchmark dirty cell count overflowed"))?;
            }
        }
        Ok(counters)
    }

    fn render_update_cell_count(update: &RenderUpdate) -> rootcause::Result<u64> {
        let rows = match update {
            RenderUpdate::Baseline(baseline) => baseline.rows(),
            RenderUpdate::Diff(diff) => diff.rows(),
        };
        rows.iter().try_fold(0_u64, |count, row| {
            count
                .checked_add(u64::try_from(row.cells().len())?)
                .ok_or_else(|| report!("muxr benchmark render cell count overflowed"))
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TerminalFingerprint {
    bytes: u64,
    fnv1a: u64,
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
    let mut mismatches = Vec::new();
    for workload in self::workloads()? {
        let first = workload.run()?;
        let second = workload.run()?;
        if first != second {
            return Err(
                report!("muxr benchmark determinism oracle mismatch").attach(format!("workload={}", workload.name))
            );
        }
        let actual = self::terminal_fingerprint(&first.terminal_bytes)?;
        if actual != workload.expected_terminal {
            mismatches.push(format!(
                "workload={} expected={:?} actual={actual:?}",
                workload.name, workload.expected_terminal
            ));
        }
    }
    if !mismatches.is_empty() {
        return Err(report!("muxr benchmark terminal oracle mismatch").attach(mismatches.join("; ")));
    }
    self::verify_edge_case_trace()
}

fn two_pane_interactive() -> rootcause::Result<Workload> {
    let size = TerminalSize::new(120, 40)?;
    let baseline = render_baseline(1, &size, None)?;
    let diff = RenderDiff::new(1, 2, size, visible_cursor(3, 7), vec![text_row(7, 0, 120, None)?])?;
    render_workload(
        "two_pane_interactive",
        vec![RenderUpdate::Baseline(baseline), RenderUpdate::Diff(diff)],
        TerminalFingerprint {
            bytes: 5_276,
            fnv1a: 5_149_238_566_510_622_795,
        },
    )
}

fn four_pane_output_burst() -> rootcause::Result<Workload> {
    let size = TerminalSize::new(160, 60)?;
    let mut updates = vec![RenderUpdate::Baseline(render_baseline(1, &size, None)?)];
    // Model an explicit 8 KiB burst as four 2 KiB pane-region updates in canonical pane order.
    for (index, (row, col)) in [(0, 0), (0, 80), (30, 0), (30, 80)].into_iter().enumerate() {
        updates.push(RenderUpdate::Diff(RenderDiff::new(
            u64::try_from(index)?.saturating_add(1),
            u64::try_from(index)?.saturating_add(2),
            size.clone(),
            visible_cursor(row, col),
            burst_rows(row, col, 2_048)?,
        )?));
    }
    render_workload(
        "four_pane_8k_output_burst",
        updates,
        TerminalFingerprint {
            bytes: 19_165,
            fnv1a: 6_454_520_309_062_025_195,
        },
    )
}

fn one_dirty_pane() -> rootcause::Result<Workload> {
    let size = TerminalSize::new(160, 60)?;
    let baseline = render_baseline(1, &size, None)?;
    let rows = (0..30)
        .map(|row| text_row(row, 0, 80, None))
        .collect::<rootcause::Result<Vec<_>>>()?;
    let diff = RenderDiff::new(1, 2, size, visible_cursor(0, 0), rows)?;
    render_workload(
        "one_dirty_pane_frame",
        vec![RenderUpdate::Baseline(baseline), RenderUpdate::Diff(diff)],
        TerminalFingerprint {
            bytes: 12_692,
            fnv1a: 3_462_938_318_298_667_905,
        },
    )
}

fn url_heavy_frame() -> rootcause::Result<Workload> {
    let size = TerminalSize::new(320, 90)?;
    render_workload(
        "url_heavy_320x90_frame",
        vec![RenderUpdate::Baseline(render_baseline(
            1,
            &size,
            Some("https://example.com/muxr/performance/reference"),
        )?)],
        TerminalFingerprint {
            bytes: 34_861,
            fnv1a: 14_501_950_852_984_424_407,
        },
    )
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
    Workload::new(
        "seven_tab_sidebar_update",
        vec![ServerEvent::SidebarLayout(layout)],
        TerminalFingerprint {
            bytes: 0,
            fnv1a: 14_695_981_039_346_656_037,
        },
    )
}

fn render_workload(
    name: &'static str,
    updates: Vec<RenderUpdate>,
    expected_terminal: TerminalFingerprint,
) -> rootcause::Result<Workload> {
    Workload::new(
        name,
        updates.into_iter().map(ServerEvent::Render).collect(),
        expected_terminal,
    )
}

fn burst_rows(row: u16, col: u16, bytes: usize) -> rootcause::Result<Vec<RenderRowSpan>> {
    const PANE_COLS: usize = 80;
    let full_rows = bytes / PANE_COLS;
    let trailing = bytes % PANE_COLS;
    let mut rows = Vec::with_capacity(full_rows.saturating_add(usize::from(trailing > 0)));
    for offset in 0..full_rows {
        rows.push(ascii_row(
            row.checked_add(u16::try_from(offset)?)
                .ok_or_else(|| report!("muxr benchmark burst row overflowed"))?,
            col,
            PANE_COLS,
        )?);
    }
    if trailing > 0 {
        rows.push(ascii_row(
            row.checked_add(u16::try_from(full_rows)?)
                .ok_or_else(|| report!("muxr benchmark burst row overflowed"))?,
            col,
            trailing,
        )?);
    }
    Ok(rows)
}

fn ascii_row(row: u16, col: u16, width: usize) -> rootcause::Result<RenderRowSpan> {
    RenderRowSpan::new(
        row,
        col,
        (0..width)
            .map(|index| {
                Ok(RenderCell::narrow(
                    char::from(b'a'.saturating_add(u8::try_from(index % 26)?)).to_string(),
                    RenderStyle::default(),
                ))
            })
            .collect::<rootcause::Result<Vec<_>>>()?,
    )
}

fn render_baseline(seq: u64, size: &TerminalSize, hyperlink: Option<&str>) -> rootcause::Result<RenderBaseline> {
    let rows = (0..size.rows())
        .map(|row| text_row(row, 0, size.cols(), hyperlink))
        .collect::<rootcause::Result<Vec<_>>>()?;
    RenderBaseline::new(seq, size.clone(), visible_cursor(0, 0), rows)
}

fn text_row(row: u16, col: u16, width: u16, hyperlink: Option<&str>) -> rootcause::Result<RenderRowSpan> {
    self::text_row_with_style(row, col, width, hyperlink, RenderStyle::default())
}

fn text_row_with_style(
    row: u16,
    col: u16,
    width: u16,
    hyperlink: Option<&str>,
    style: RenderStyle,
) -> rootcause::Result<RenderRowSpan> {
    let mut cells = Vec::with_capacity(usize::from(width));
    for index in 0..width {
        let cell = if index == 0 && row == 0 && width >= 2 {
            RenderCell::wide("表", style)
        } else if index == 1 && row == 0 && width >= 2 {
            RenderCell::wide_continuation(style)
        } else {
            RenderCell::narrow(
                char::from(b'a'.saturating_add(u8::try_from(index % 26)?)).to_string(),
                style,
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
        // A distinct full-row style change models focus/attention recomposition at the client protocol boundary.
        ServerEvent::Render(RenderUpdate::Diff(RenderDiff::new(
            1,
            2,
            size.clone(),
            visible_cursor(0, 0),
            vec![text_row_with_style(
                0,
                0,
                8,
                None,
                RenderStyle {
                    attrs: RenderTextStyle::empty().set_bold(true),
                    bg: RenderColor::Indexed(1),
                    fg: RenderColor::Default,
                },
            )?],
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
    let workload = Workload::new(
        "edge_case_trace",
        trace,
        TerminalFingerprint {
            bytes: 264,
            fnv1a: 9_051_240_227_306_872_163,
        },
    )?;
    let result = workload.run()?;
    let actual = self::terminal_fingerprint(&result.terminal_bytes)?;
    if actual != workload.expected_terminal {
        return Err(report!("muxr benchmark edge-case terminal oracle mismatch")
            .attach(format!("expected={:?}", workload.expected_terminal))
            .attach(format!("actual={actual:?}")));
    }
    self::verify_resynchronization_oracle()
}

fn terminal_fingerprint(bytes: &[u8]) -> rootcause::Result<TerminalFingerprint> {
    let mut fnv1a = 14_695_981_039_346_656_037_u64;
    for byte in bytes {
        fnv1a ^= u64::from(*byte);
        fnv1a = fnv1a.wrapping_mul(1_099_511_628_211);
    }
    Ok(TerminalFingerprint {
        bytes: u64::try_from(bytes.len())?,
        fnv1a,
    })
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_benchmark_workloads_when_repeated_match_behavioral_oracle() -> rootcause::Result<()> {
        verify_oracle()
    }

    #[test]
    fn test_four_pane_output_burst_when_run_encodes_four_two_kib_dirty_regions() -> rootcause::Result<()> {
        let result = four_pane_output_burst()?.run()?;

        assert_that!(result.counters.dirty_cells_encoded, eq(8_192));
        assert_that!(result.counters.frames_encoded, eq(5));
        assert_that!(result.counters.render_cells_encoded, eq(17_792));
        Ok(())
    }
}
