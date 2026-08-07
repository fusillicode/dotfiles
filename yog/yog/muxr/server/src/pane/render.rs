use std::collections::BTreeMap;
use std::sync::Arc;

use muxr_config::PaneAttentionConfig;
use muxr_config::PaneBorderStyles;
use muxr_config::PaneDimConfig;
use muxr_core::PaneId;
use muxr_core::RenderBaseline;
use muxr_core::RenderCell;
use muxr_core::RenderColor;
use muxr_core::RenderCursor;
use muxr_core::RenderDiff;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderUpdate;
use muxr_core::TerminalSize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use smallvec::SmallVec;

use crate::pane::borders::BorderRenderMode;
use crate::pane::layout::PaneLayout;
use crate::pane::layout::PaneRegion;
use crate::pty::PtyRenderSnapshot;
use crate::render_state::ClientRenderDmg;
use crate::terminal::TerminalSnapshot;
use crate::terminal::TerminalSnapshotScope;

struct CompositeFrame {
    active_pane: PaneId,
    attention_panes: Vec<PaneId>,
    cursor: RenderCursor,
    pane_layout: Arc<PaneLayout>,
    pane_render: PaneRenderConfig,
    pane_snapshots: BTreeMap<PaneId, PtyRenderSnapshot>,
    rows: Vec<Vec<RenderCell>>,
    scratch_rows: Vec<Vec<RenderCell>>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneRenderConfig {
    pub mode: BorderRenderMode,
    pub border_styles: PaneBorderStyles,
    pub pane_attention: PaneAttentionConfig,
    pub pane_dim: PaneDimConfig,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneRenderLayout<'a> {
    pub active_pane: PaneId,
    pub pane_layout: &'a PaneLayout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneVisualRole {
    Normal,
    Unfocused,
    Attention,
}

impl PaneVisualRole {
    fn for_pane(pane_id: PaneId, active_pane: PaneId, attention_panes: &[PaneId]) -> Self {
        if pane_id == active_pane {
            return Self::Normal;
        }
        if attention_panes.contains(&pane_id) {
            return Self::Attention;
        }
        Self::Unfocused
    }

    const fn style(self, pane_dim: PaneDimConfig, pane_attention: PaneAttentionConfig) -> PaneVisualStyle {
        let dim = match self {
            Self::Unfocused | Self::Attention if pane_dim.unfocused => Some(pane_dim),
            Self::Normal | Self::Unfocused | Self::Attention => None,
        };
        let bg_tint = match self {
            Self::Attention => pane_attention.bg_tint,
            Self::Normal | Self::Unfocused => None,
        };
        PaneVisualStyle {
            attention_bg_tint: bg_tint,
            dim,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PaneVisualStyle {
    attention_bg_tint: Option<RenderColor>,
    dim: Option<PaneDimConfig>,
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
    pub const fn has_baseline(&self) -> bool {
        self.last_sent.is_some()
    }

    fn render_frame_baseline(&mut self, mut frame: CompositeFrame) -> rootcause::Result<RenderUpdate> {
        frame.seq = self.next_sequence()?;
        let baseline = RenderBaseline::new(
            frame.seq,
            frame.size.clone(),
            frame.cursor.clone(),
            render_row_spans(&frame.rows, 0..frame.rows.len())?,
        )?;
        self.last_sent = Some(frame);
        Ok(RenderUpdate::Baseline(baseline))
    }

    pub fn render_baseline_with_snapshot(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        mut snapshot: impl FnMut(PaneId, TerminalSnapshotScope) -> rootcause::Result<PtyRenderSnapshot>,
    ) -> rootcause::Result<RenderUpdate> {
        self.render_frame_baseline(Self::current_frame_with(
            pane_render,
            pane_layout,
            size,
            attention_panes,
            &mut snapshot,
        )?)
    }

    pub fn render_diff_with_snapshot(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        damage: &ClientRenderDmg,
        mut snapshot: impl FnMut(PaneId, TerminalSnapshotScope) -> rootcause::Result<PtyRenderSnapshot>,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let Some(previous) = self.last_sent.as_ref() else {
            let frame = Self::current_frame_with(pane_render, pane_layout, size, attention_panes, &mut snapshot)?;
            return Ok(Some(self.render_frame_baseline(frame)?));
        };
        let cache_matches = previous.size == *size
            && previous.pane_render == pane_render
            && previous.pane_layout.as_ref() == pane_layout.pane_layout
            && previous.active_pane == pane_layout.active_pane
            && previous.attention_panes == attention_panes;
        let damage_matches_cache = match damage {
            ClientRenderDmg::Panes(pane_ids) | ClientRenderDmg::RegionChanged(pane_ids) => pane_ids
                .iter()
                .all(|pane_id| previous.pane_snapshots.contains_key(pane_id)),
            ClientRenderDmg::Clean | ClientRenderDmg::Full => true,
        };
        if !cache_matches || !damage_matches_cache {
            // A style/focus/attention miss still has valid immutable geometry; retain its ownership while refreshing
            // every pane snapshot so true fallback does not pay to deep-clone an unchanged layout.
            let owned_layout = if previous.pane_layout.as_ref() == pane_layout.pane_layout {
                Arc::clone(&previous.pane_layout)
            } else {
                Arc::new(pane_layout.pane_layout.clone())
            };
            let frame = Self::current_frame_with_layout(
                pane_render,
                pane_layout.active_pane,
                owned_layout,
                size,
                attention_panes,
                &mut snapshot,
            )?;
            if frame.size != previous.size {
                return Ok(Some(self.render_frame_baseline(frame)?));
            }
            let reason = if matches!(damage, ClientRenderDmg::RegionChanged(_)) {
                RenderDiffReason::RegionChanged
            } else {
                RenderDiffReason::DirtyFrame
            };
            return self.render_frame_diff(frame, reason);
        }
        if matches!(damage, ClientRenderDmg::Full) {
            let pane_ids = previous
                .pane_layout
                .regions()
                .iter()
                .map(|region| region.id)
                .collect::<SmallVec<[_; 4]>>();
            return self.render_cached_full_diff_with(&pane_ids, RenderDiffReason::DirtyFrame, snapshot);
        }

        let (pane_ids, reason) = match damage {
            ClientRenderDmg::Clean => return Ok(None),
            ClientRenderDmg::Panes(pane_ids) => (pane_ids, RenderDiffReason::DirtyFrame),
            ClientRenderDmg::RegionChanged(pane_ids) => (pane_ids, RenderDiffReason::RegionChanged),
            ClientRenderDmg::Full => return Err(report!("muxr full render bypassed its cached composer path")),
        };
        self.render_pane_diff_with(pane_ids, reason, snapshot)
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
            let rows = previous.rows.iter().zip(frame.rows.iter()).enumerate().filter_map(
                |(row, (previous_row, current_row))| (previous_row != current_row).then_some((row, current_row)),
            );
            let rows = render_row_spans_from_pairs(rows)?;
            (previous.seq, frame.cursor != previous.cursor, rows)
        };
        if rows.is_empty() && !cursor_changed && reason == RenderDiffReason::DirtyFrame {
            // Pixel-identical damage can still refresh cached pane metadata used by the following PaneRegions event.
            frame.seq = previous_seq;
            self.last_sent = Some(frame);
            return Ok(None);
        }

        frame.seq = self.next_sequence()?;
        let diff = RenderDiff::new(previous_seq, frame.seq, frame.size.clone(), frame.cursor.clone(), rows)?;
        self.last_sent = Some(frame);
        Ok(Some(RenderUpdate::Diff(diff)))
    }

    fn render_pane_diff_with(
        &mut self,
        pane_ids: &[PaneId],
        reason: RenderDiffReason,
        mut snapshot: impl FnMut(PaneId, TerminalSnapshotScope) -> rootcause::Result<PtyRenderSnapshot>,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let (previous_seq, cursor, rows, size) = {
            let frame = self
                .last_sent
                .as_mut()
                .ok_or_else(|| report!("muxr partial render is missing its baseline"))?;
            let previous = refresh_pane_rows_with(frame, pane_ids, &mut snapshot)?;
            let changed_rows = previous.rows.iter().filter_map(|(row, previous_row)| {
                let current = frame.rows.get(usize::from(*row))?;
                (previous_row != current).then_some((usize::from(*row), current))
            });
            let rows = render_row_spans_from_pairs(changed_rows)?;
            let cursor_changed = frame.cursor != previous.cursor;
            frame
                .scratch_rows
                .extend(previous.rows.into_iter().map(|(_row, cells)| cells));
            if rows.is_empty() && !cursor_changed && reason == RenderDiffReason::DirtyFrame {
                return Ok(None);
            }
            (frame.seq, frame.cursor.clone(), rows, frame.size.clone())
        };
        let seq = self.next_sequence()?;
        self.last_sent
            .as_mut()
            .ok_or_else(|| report!("muxr partial render lost its baseline"))?
            .seq = seq;
        Ok(Some(RenderUpdate::Diff(RenderDiff::new(
            previous_seq,
            seq,
            size,
            cursor,
            rows,
        )?)))
    }

    fn render_cached_full_diff_with(
        &mut self,
        pane_ids: &[PaneId],
        reason: RenderDiffReason,
        mut snapshot: impl FnMut(PaneId, TerminalSnapshotScope) -> rootcause::Result<PtyRenderSnapshot>,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let (active_pane, attention_panes, pane_layout, pane_render, mut pane_snapshots, size) = {
            let frame = self
                .last_sent
                .as_mut()
                .ok_or_else(|| report!("muxr cached full render is missing its baseline"))?;
            (
                frame.active_pane,
                frame.attention_panes.clone(),
                Arc::clone(&frame.pane_layout),
                frame.pane_render,
                std::mem::take(&mut frame.pane_snapshots),
                frame.size.clone(),
            )
        };
        for pane_id in pane_ids {
            let update = snapshot(*pane_id, TerminalSnapshotScope::ChangedRows)?;
            pane_snapshots
                .get_mut(pane_id)
                .ok_or_else(|| report!("muxr cached full render is missing a pane snapshot"))?
                .apply_update(update)?;
        }
        let frame = Self::frame_from_snapshots(
            pane_render,
            active_pane,
            pane_layout,
            pane_snapshots,
            &size,
            &attention_panes,
        )?;
        self.render_frame_diff(frame, reason)
    }

    fn current_frame_with(
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        snapshot: &mut impl FnMut(PaneId, TerminalSnapshotScope) -> rootcause::Result<PtyRenderSnapshot>,
    ) -> rootcause::Result<CompositeFrame> {
        Self::current_frame_with_layout(
            pane_render,
            pane_layout.active_pane,
            Arc::new(pane_layout.pane_layout.clone()),
            size,
            attention_panes,
            snapshot,
        )
    }

    fn current_frame_with_layout(
        pane_render: PaneRenderConfig,
        active_pane: PaneId,
        pane_layout: Arc<PaneLayout>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        snapshot: &mut impl FnMut(PaneId, TerminalSnapshotScope) -> rootcause::Result<PtyRenderSnapshot>,
    ) -> rootcause::Result<CompositeFrame> {
        let mut pane_snapshots = BTreeMap::new();
        for region in pane_layout.regions() {
            pane_snapshots.insert(region.id, snapshot(region.id, TerminalSnapshotScope::Full)?);
        }
        Self::frame_from_snapshots(
            pane_render,
            active_pane,
            pane_layout,
            pane_snapshots,
            size,
            attention_panes,
        )
    }

    fn frame_from_snapshots(
        pane_render: PaneRenderConfig,
        active_pane: PaneId,
        pane_layout: Arc<PaneLayout>,
        pane_snapshots: BTreeMap<PaneId, PtyRenderSnapshot>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
    ) -> rootcause::Result<CompositeFrame> {
        let mut rows = empty_render_rows(size);
        for region in pane_layout.regions() {
            let snapshot = pane_snapshots
                .get(&region.id)
                .ok_or_else(|| report!("muxr full composer is missing a pane snapshot"))?;
            let visual_role = PaneVisualRole::for_pane(region.id, active_pane, attention_panes);
            paste_snapshot(
                &mut rows,
                region,
                snapshot.terminal(),
                visual_role.style(pane_render.pane_dim, pane_render.pane_attention),
            )?;
        }
        crate::pane::borders::paste_borders(
            &mut rows,
            pane_render.border_styles,
            pane_render.pane_attention,
            pane_layout.borders(),
            Some(&active_pane),
            attention_panes,
            pane_render.mode,
        )?;

        Ok(CompositeFrame {
            active_pane,
            attention_panes: attention_panes.to_vec(),
            cursor: composite_cursor(active_pane, &pane_layout, &pane_snapshots)?,
            pane_layout,
            pane_render,
            pane_snapshots,
            rows,
            scratch_rows: Vec::new(),
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct AffectedRows {
    len: usize,
    rows: Vec<AffectedRow>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AffectedRow {
    Affected,
    Unaffected,
}

impl AffectedRows {
    fn for_pane_rows(
        pane_layout: &PaneLayout,
        pane_rows: &[(PaneId, Vec<u16>)],
        frame_rows: usize,
    ) -> rootcause::Result<Self> {
        let mut affected_rows = vec![AffectedRow::Unaffected; frame_rows];
        let mut len = 0_usize;
        for (pane_id, changed_rows) in pane_rows {
            let region = pane_layout
                .regions()
                .iter()
                .find(|region| region.id == *pane_id)
                .ok_or_else(|| report!("muxr pane damage is outside the cached visible layout"))?;
            for pane_row in changed_rows {
                let start = region
                    .area
                    .origin
                    .row
                    .checked_add(*pane_row)
                    .ok_or_else(|| report!("muxr pane damage row overflowed"))?;
                let row = affected_rows
                    .get_mut(usize::from(start))
                    .ok_or_else(|| report!("muxr pane damage row is outside the composite frame"))?;
                if *row == AffectedRow::Unaffected {
                    *row = AffectedRow::Affected;
                    len = len.saturating_add(1);
                }
            }
        }
        Ok(Self {
            len,
            rows: affected_rows,
        })
    }

    fn contains(&self, row: u16) -> bool {
        self.rows.get(usize::from(row)) == Some(&AffectedRow::Affected)
    }

    fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.rows.iter().enumerate().filter_map(|(row, state)| {
            (*state == AffectedRow::Affected)
                .then(|| u16::try_from(row).ok())
                .flatten()
        })
    }

    const fn len(&self) -> usize {
        self.len
    }

    fn pane_rows(&self, region: &PaneRegion) -> rootcause::Result<Vec<u16>> {
        let start = region.area.origin.row;
        let end = start
            .checked_add(region.area.size.rows)
            .ok_or_else(|| report!("muxr pane affected-row range overflowed"))?;
        self.iter()
            .filter(|row| *row >= start && *row < end)
            .map(|row| {
                row.checked_sub(start)
                    .ok_or_else(|| report!("muxr pane affected-row offset underflowed"))
            })
            .collect()
    }
}

struct PartialFrameBefore {
    affected_rows: AffectedRows,
    cursor: RenderCursor,
    rows: Vec<(u16, Vec<RenderCell>)>,
}

fn refresh_pane_rows_with(
    frame: &mut CompositeFrame,
    pane_ids: &[PaneId],
    snapshot: &mut impl FnMut(PaneId, TerminalSnapshotScope) -> rootcause::Result<PtyRenderSnapshot>,
) -> rootcause::Result<PartialFrameBefore> {
    let mut pane_rows = Vec::with_capacity(pane_ids.len());
    for pane_id in pane_ids {
        let update = snapshot(*pane_id, TerminalSnapshotScope::ChangedRows)?;
        let changed_rows = update
            .terminal()
            .rows()
            .iter()
            .map(muxr_core::RenderRowSpan::row)
            .collect::<Vec<_>>();
        let pane_snapshot = frame
            .pane_snapshots
            .get_mut(pane_id)
            .ok_or_else(|| report!("muxr partial composer is missing a cached pane snapshot"))?;
        let mut affected_pane_rows = crate::pane::url_links::expand_visible_url_rows(
            pane_snapshot.terminal().rows(),
            pane_snapshot.terminal().row_wraps(),
            &changed_rows,
        );
        let applied_rows = pane_snapshot.apply_update(update)?;
        affected_pane_rows.extend(crate::pane::url_links::expand_visible_url_rows(
            pane_snapshot.terminal().rows(),
            pane_snapshot.terminal().row_wraps(),
            &applied_rows,
        ));
        affected_pane_rows.extend(applied_rows);
        affected_pane_rows.sort_unstable();
        affected_pane_rows.dedup();
        pane_rows.push((*pane_id, affected_pane_rows));
    }
    let affected_rows = AffectedRows::for_pane_rows(&frame.pane_layout, &pane_rows, frame.rows.len())?;
    let mut previous_rows = Vec::with_capacity(affected_rows.len());
    let blank = RenderCell::narrow(" ", RenderStyle::default());
    for row in affected_rows.iter() {
        let cells = frame
            .rows
            .get_mut(usize::from(row))
            .ok_or_else(|| report!("muxr damaged row is outside the cached composite frame"))?;
        let mut scratch = frame.scratch_rows.pop().unwrap_or_default();
        scratch.clear();
        scratch.resize(cells.len(), blank.clone());
        std::mem::swap(cells, &mut scratch);
        previous_rows.push((row, scratch));
    }
    let previous = PartialFrameBefore {
        affected_rows,
        cursor: frame.cursor.clone(),
        rows: previous_rows,
    };
    // Pane regions can overlap at shared edges. Recompose only the affected rows, in canonical layout order,
    // so a changed pane cannot overwrite a later pane or its border without snapshotting unchanged panes.
    for region in frame.pane_layout.regions() {
        let selected_rows = previous.affected_rows.pane_rows(region)?;
        if selected_rows.is_empty() {
            continue;
        }
        let snapshot = frame
            .pane_snapshots
            .get(&region.id)
            .ok_or_else(|| report!("muxr partial composer is missing a cached pane snapshot"))?;
        let visual_role = PaneVisualRole::for_pane(region.id, frame.active_pane, &frame.attention_panes);
        paste_snapshot_rows(
            &mut frame.rows,
            region,
            snapshot.terminal(),
            visual_role.style(frame.pane_render.pane_dim, frame.pane_render.pane_attention),
            Some(&selected_rows),
        )?;
    }
    crate::pane::borders::paste_borders_in_rows(
        &mut frame.rows,
        crate::pane::borders::PasteBordersConfig {
            active_pane: Some(&frame.active_pane),
            attention_panes: &frame.attention_panes,
            border_mode: frame.pane_render.mode,
            borders: frame.pane_layout.borders(),
            pane_attention: frame.pane_render.pane_attention,
            styles: frame.pane_render.border_styles,
        },
        |row| previous.affected_rows.contains(row),
    )?;
    frame.cursor = composite_cursor(frame.active_pane, &frame.pane_layout, &frame.pane_snapshots)?;
    Ok(previous)
}

fn empty_render_rows(size: &TerminalSize) -> Vec<Vec<RenderCell>> {
    let blank = RenderCell::narrow(" ", RenderStyle::default());
    (0..size.rows())
        .map(|_| vec![blank.clone(); usize::from(size.cols())])
        .collect()
}

fn composite_cursor(
    active_pane: PaneId,
    pane_layout: &PaneLayout,
    pane_snapshots: &BTreeMap<PaneId, PtyRenderSnapshot>,
) -> rootcause::Result<RenderCursor> {
    let hidden = RenderCursor {
        row: 0,
        col: 0,
        shape: muxr_core::RenderCursorShape::Default,
        visibility: muxr_core::RenderCursorVisibility::Hidden,
    };
    let Some(region) = pane_layout.regions().iter().find(|region| region.id == active_pane) else {
        return Ok(hidden);
    };
    let snapshot = pane_snapshots
        .get(&active_pane)
        .ok_or_else(|| report!("muxr active pane is missing its render snapshot"))?
        .terminal();
    if snapshot.cursor().visibility != muxr_core::RenderCursorVisibility::Visible {
        return Ok(hidden);
    }
    Ok(RenderCursor {
        row: region
            .area
            .origin
            .row
            .checked_add(snapshot.cursor().row)
            .ok_or_else(|| report!("muxr composite cursor row overflowed"))?,
        col: region
            .area
            .origin
            .col
            .checked_add(snapshot.cursor().col)
            .ok_or_else(|| report!("muxr composite cursor col overflowed"))?,
        shape: snapshot.cursor().shape,
        visibility: muxr_core::RenderCursorVisibility::Visible,
    })
}

fn render_row_spans(
    rows: &[Vec<RenderCell>],
    indices: impl IntoIterator<Item = usize>,
) -> rootcause::Result<Vec<RenderRowSpan>> {
    render_row_spans_from_pairs(
        indices
            .into_iter()
            .filter_map(|row| rows.get(row).map(|cells| (row, cells))),
    )
}

fn render_row_spans_from_pairs<'a>(
    rows: impl IntoIterator<Item = (usize, &'a Vec<RenderCell>)>,
) -> rootcause::Result<Vec<RenderRowSpan>> {
    rows.into_iter()
        .map(|(row, cells)| {
            RenderRowSpan::new(
                u16::try_from(row).context("muxr composite render row overflowed")?,
                0,
                cells.clone(),
            )
        })
        .collect()
}

fn paste_snapshot(
    rows: &mut [Vec<RenderCell>],
    region: &PaneRegion,
    snapshot: &TerminalSnapshot,
    visual_style: PaneVisualStyle,
) -> rootcause::Result<()> {
    paste_snapshot_rows(rows, region, snapshot, visual_style, None)
}

fn paste_snapshot_rows(
    rows: &mut [Vec<RenderCell>],
    region: &PaneRegion,
    snapshot: &TerminalSnapshot,
    visual_style: PaneVisualStyle,
    selected_rows: Option<&[u16]>,
) -> rootcause::Result<()> {
    if snapshot.size().cols() != region.area.size.cols || snapshot.size().rows() != region.area.size.rows {
        return Err(report!("muxr pane snapshot size does not match region")
            .attach(format!("pane_id={}", region.id))
            .attach(format!("snapshot_cols={}", snapshot.size().cols()))
            .attach(format!("snapshot_rows={}", snapshot.size().rows()))
            .attach(format!("region_cols={}", region.area.size.cols))
            .attach(format!("region_rows={}", region.area.size.rows)));
    }

    let url_links = if let Some(selected_rows) = selected_rows {
        crate::pane::url_links::detect_visible_url_links_for_rows(snapshot.rows(), snapshot.row_wraps(), selected_rows)?
    } else {
        crate::pane::url_links::detect_visible_url_links(snapshot.rows(), snapshot.row_wraps())?
    };
    let mut url_links = url_links.into_iter().peekable();
    if let Some(selected_rows) = selected_rows {
        for selected_row in selected_rows {
            let span_index = usize::from(*selected_row);
            let span = snapshot
                .rows()
                .get(span_index)
                .ok_or_else(|| report!("muxr selected pane row is outside its cached snapshot"))?;
            self::paste_snapshot_row(rows, region, span, span_index, visual_style, &mut url_links)?;
        }
        return Ok(());
    }
    for (span_index, span) in snapshot.rows().iter().enumerate() {
        self::paste_snapshot_row(rows, region, span, span_index, visual_style, &mut url_links)?;
    }
    Ok(())
}

fn paste_snapshot_row(
    rows: &mut [Vec<RenderCell>],
    region: &PaneRegion,
    span: &RenderRowSpan,
    span_index: usize,
    visual_style: PaneVisualStyle,
    url_links: &mut std::iter::Peekable<std::vec::IntoIter<crate::pane::url_links::PaneUrlLink>>,
) -> rootcause::Result<()> {
    while url_links.peek().is_some_and(|link| link.row() < span_index) {
        let _skipped = url_links.next();
    }
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
        let mut cell = cell
            .clone()
            .with_style(self::pane_visual_render_style(cell.style(), visual_style));
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
    Ok(())
}

fn pane_visual_render_style(mut style: RenderStyle, visual_style: PaneVisualStyle) -> RenderStyle {
    if let Some(pane_dim) = visual_style.dim {
        style = crate::pane::dim::apply_dim_style(style, pane_dim);
    }
    if let Some(bg_tint) = visual_style.attention_bg_tint {
        style = crate::pane::attention::apply_attention_tint(style, bg_tint);
    }
    style
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use test_that::prelude::*;

    use super::*;
    use crate::pane::layout::PaneArea;
    use crate::pane::layout::PanePosition;
    use crate::pane::layout::PaneSize;

    #[rstest::rstest]
    #[case::dirty_frame(RenderDiffReason::DirtyFrame, ExpectedRenderDiff::None)]
    #[case::region_changed(RenderDiffReason::RegionChanged, ExpectedRenderDiff::Diff)]
    fn test_render_composer_render_frame_diff_when_pixels_are_unchanged_respects_reason(
        #[case] reason: RenderDiffReason,
        #[case] expected_diff: ExpectedRenderDiff,
    ) -> rootcause::Result<()> {
        let size = TerminalSize::new(2, 1)?;
        let cursor = RenderCursor {
            row: 0,
            col: 0,
            shape: muxr_core::RenderCursorShape::Default,
            visibility: muxr_core::RenderCursorVisibility::Hidden,
        };
        let rows = vec![vec![
            RenderCell::narrow("a", RenderStyle::default()),
            RenderCell::narrow("b", RenderStyle::default()),
        ]];
        let config = MuxrConfig::default();
        let pane_render = PaneRenderConfig {
            border_styles: config.pane_borders,
            mode: BorderRenderMode::Focus,
            pane_attention: config.pane_attention,
            pane_dim: config.pane_dim,
        };
        let previous = CompositeFrame {
            active_pane: PaneId::new(1)?,
            attention_panes: Vec::new(),
            cursor: cursor.clone(),
            pane_layout: Arc::new(PaneLayout::default()),
            pane_render,
            pane_snapshots: BTreeMap::new(),
            rows: rows.clone(),
            scratch_rows: Vec::new(),
            seq: 1,
            size: size.clone(),
        };
        let current = CompositeFrame {
            active_pane: PaneId::new(1)?,
            attention_panes: Vec::new(),
            cursor,
            pane_layout: Arc::new(PaneLayout::default()),
            pane_render,
            pane_snapshots: BTreeMap::new(),
            rows,
            scratch_rows: Vec::new(),
            seq: 0,
            size,
        };
        let mut composer = RenderComposer {
            last_sent: Some(previous),
            next_seq: 2,
        };

        let update = composer.render_frame_diff(current, reason)?;

        if expected_diff == ExpectedRenderDiff::None {
            assert_that!(update, eq(None));
            assert_that!(composer.next_seq, eq(2));
            assert_that!(composer.last_sent.as_ref().map(|frame| frame.seq), eq(Some(1)));
            return Ok(());
        }

        let Some(RenderUpdate::Diff(diff)) = update else {
            return Err(report!("expected muxr region-change diff"));
        };
        assert_that!(diff.base_seq(), eq(1));
        assert_that!(diff.seq(), eq(2));
        assert_that!(diff.rows(), points_to(empty()));
        Ok(())
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ExpectedRenderDiff {
        Diff,
        None,
    }

    #[rstest::rstest]
    #[case::active_pane(1, &[2], PaneVisualRole::Normal)]
    #[case::unfocused_pane(2, &[], PaneVisualRole::Unfocused)]
    #[case::attention_pane(2, &[2], PaneVisualRole::Attention)]
    #[case::active_attention_pane(1, &[1], PaneVisualRole::Normal)]
    fn test_pane_visual_role_when_focus_and_attention_vary_selects_semantic_role(
        #[case] pane_id: u32,
        #[case] attention_panes: &[u32],
        #[case] expected: PaneVisualRole,
    ) -> rootcause::Result<()> {
        let attention_panes = attention_panes
            .iter()
            .map(|pane_id| PaneId::new(*pane_id))
            .collect::<rootcause::Result<Vec<_>>>()?;

        assert_that!(
            PaneVisualRole::for_pane(PaneId::new(pane_id)?, PaneId::new(1)?, &attention_panes),
            eq(expected)
        );
        Ok(())
    }

    #[test]
    fn test_affected_rows_when_one_pane_row_changes_covers_only_changed_row() -> rootcause::Result<()> {
        let pane_id = PaneId::new(1)?;
        let layout = PaneLayout::single_pane(pane_id, 1, &TerminalSize::new(8, 5)?);

        let affected = AffectedRows::for_pane_rows(&layout, &[(pane_id, vec![2])], 5)?;

        assert_that!(affected.iter().collect::<Vec<_>>(), eq(vec![2]));
        Ok(())
    }

    #[test]
    fn test_pane_visual_render_style_when_normal_keeps_style_unchanged() {
        let style = RenderStyle {
            attrs: muxr_core::RenderTextStyle::empty().set_bold(true),
            bg: RenderColor::Rgb { r: 20, g: 20, b: 20 },
            fg: RenderColor::Indexed(7),
        };

        let updated = self::pane_visual_render_style(
            style,
            PaneVisualStyle {
                attention_bg_tint: None,
                dim: None,
            },
        );

        assert_that!(updated, eq(style));
    }

    #[test]
    fn test_pane_visual_render_style_when_attention_tints_rgb_bg_and_darkens_explicit_fg() {
        let style = RenderStyle {
            attrs: muxr_core::RenderTextStyle::empty().set_italic(true),
            bg: RenderColor::Rgb { r: 20, g: 20, b: 20 },
            fg: RenderColor::Indexed(7),
        };

        let updated = self::pane_visual_render_style(
            style,
            PaneVisualStyle {
                attention_bg_tint: Some(RenderColor::Rgb { r: 80, g: 0, b: 0 }),
                dim: Some(PaneDimConfig {
                    explicit_color_percent: 80,
                    unfocused: true,
                }),
            },
        );

        assert_that!(updated.attrs.italic(), eq(true));
        assert_that!(updated.attrs.dim(), eq(false));
        assert_that!(updated.bg, not(eq(style.bg)));
        assert_that!(updated.fg, not(eq(style.fg)));
    }

    #[test]
    fn test_paste_snapshot_when_visible_url_is_present_adds_hyperlink_metadata() -> rootcause::Result<()> {
        let size = TerminalSize::new(24, 1)?;
        let mut terminal = crate::terminal::TerminalState::with_scrollback(&size, MuxrConfig::default().scrollback);
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

        paste_snapshot(
            &mut rows,
            &region,
            &snapshot,
            PaneVisualStyle {
                attention_bg_tint: None,
                dim: Some(PaneDimConfig {
                    explicit_color_percent: 80,
                    unfocused: true,
                }),
            },
        )?;

        let row = rows.first().ok_or_else(|| report!("expected muxr composite row"))?;
        let linked_cells = row.iter().filter(|cell| cell.hyperlink().is_some()).collect::<Vec<_>>();
        let linked_text = linked_cells.iter().map(|cell| cell.text()).collect::<String>();
        assert_that!(linked_text, eq("https://example.com"));
        for cell in linked_cells {
            assert_that!(cell.style().attrs.dim(), eq(true));
            assert_that!(
                cell.hyperlink().map(muxr_core::RenderHyperlink::uri),
                eq(Some("https://example.com"))
            );
        }
        Ok(())
    }

    #[test]
    fn test_paste_snapshot_when_osc8_label_is_url_preserves_explicit_target() -> rootcause::Result<()> {
        let row =
            self::paste_terminal_row(b"\x1b]8;;https://redirect.example/id\x1b\\https://docs.example\x1b]8;;\x1b\\")?;

        let linked_cells = row.iter().filter(|cell| cell.hyperlink().is_some()).collect::<Vec<_>>();
        assert_that!(
            linked_cells.iter().map(|cell| cell.text()).collect::<String>(),
            eq("https://docs.example")
        );
        assert_that!(
            linked_cells.iter().all(|cell| {
                cell.hyperlink().map(muxr_core::RenderHyperlink::uri) == Some("https://redirect.example/id")
            }),
            eq(true)
        );
        Ok(())
    }

    #[test]
    fn test_paste_snapshot_when_osc8_overlaps_url_does_not_auto_link_remainder() -> rootcause::Result<()> {
        let row =
            self::paste_terminal_row(b"\x1b]8;;https://redirect.example/id\x1b\\https\x1b]8;;\x1b\\://docs.example")?;

        let linked_cells = row.iter().filter(|cell| cell.hyperlink().is_some()).collect::<Vec<_>>();
        assert_that!(
            linked_cells.iter().map(|cell| cell.text()).collect::<String>(),
            eq("https")
        );
        assert_that!(
            linked_cells.iter().all(|cell| {
                cell.hyperlink().map(muxr_core::RenderHyperlink::uri) == Some("https://redirect.example/id")
            }),
            eq(true)
        );
        Ok(())
    }

    fn paste_terminal_row(bytes: &[u8]) -> rootcause::Result<Vec<RenderCell>> {
        let size = TerminalSize::new(24, 1)?;
        let mut terminal = crate::terminal::TerminalState::with_scrollback(&size, MuxrConfig::default().scrollback);
        let _ = terminal.process(bytes);
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
        paste_snapshot(
            &mut rows,
            &region,
            &snapshot,
            PaneVisualStyle {
                attention_bg_tint: None,
                dim: None,
            },
        )?;
        rows.into_iter()
            .next()
            .ok_or_else(|| report!("expected muxr composite row"))
    }
}
