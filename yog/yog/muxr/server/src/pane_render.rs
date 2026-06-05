use muxr_core::PaneId;
use muxr_core::RenderBaseline;
use muxr_core::RenderCell;
use muxr_core::RenderCursor;
use muxr_core::RenderDiff;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderUpdate;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::pane_borders::BorderRenderMode;
use crate::pane_layout::PaneRegion;
use crate::pane_runtime::PaneRuntimes;
use crate::state::SessionLayout;
use crate::terminal::TerminalSnapshot;

struct CompositeFrame {
    cursor: RenderCursor,
    rows: Vec<RenderRowSpan>,
    seq: u64,
    size: TerminalSize,
}

pub struct RenderComposer {
    last_sent: Option<CompositeFrame>,
    next_seq: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderDiffReason {
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
    pub fn render_baseline(
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

    pub fn render_diff(
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
        let mut rows = empty_render_rows(size);
        let mut cursor = RenderCursor {
            row: 0,
            col: 0,
            visible: false,
        };

        for region in pane_layout.regions() {
            let snapshot = runtimes.snapshot(region.id)?;
            paste_snapshot(&mut rows, region, &snapshot)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane_layout::PaneArea;
    use crate::pane_layout::PanePosition;
    use crate::pane_layout::PaneSize;

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
            area: PaneArea {
                origin: PanePosition { row: 0, col: 0 },
                size: PaneSize { rows: 1, cols: 24 },
            },
            focus_seq: 1,
            id: PaneId::new(1)?,
        };
        let mut rows = empty_render_rows(&size);

        paste_snapshot(&mut rows, &region, &snapshot)?;

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
}
