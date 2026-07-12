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
use crate::pane::runtime::PaneRuntimes;
use crate::pty::PtyRenderSnapshot;
use crate::render_state::ClientRenderDmg;
use crate::terminal::TerminalSnapshot;
#[cfg(feature = "benchmarking")]
use crate::terminal::TerminalState;

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

#[cfg(feature = "benchmarking")]
pub struct BenchmarkRenderChange {
    pub damage: ClientRenderDmg,
    pub reason: RenderDiffReason,
    pub visible_top_row: u64,
}

#[cfg(feature = "benchmarking")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BenchmarkPaneRegion {
    mouse_mode: muxr_core::PaneMouseMode,
    pane_id: PaneId,
    visible_row_wraps: Vec<muxr_core::RowWrap>,
    visible_top_row: u64,
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
    #[cfg(feature = "benchmarking")]
    pub(crate) fn benchmark_baseline(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        snapshots: &BTreeMap<PaneId, TerminalSnapshot>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        visible_top_row: u64,
    ) -> rootcause::Result<RenderUpdate> {
        let snapshots = benchmark_pane_snapshots(snapshots, pane_layout.active_pane, visible_top_row);
        self.render_frame_baseline(Self::frame_from_snapshots(
            pane_render,
            pane_layout.active_pane,
            Arc::new(pane_layout.pane_layout.clone()),
            snapshots,
            size,
            attention_panes,
        )?)
    }

    #[cfg(feature = "benchmarking")]
    pub(crate) fn benchmark_diff(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        snapshots: &BTreeMap<PaneId, TerminalSnapshot>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        change: BenchmarkRenderChange,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let BenchmarkRenderChange {
            damage,
            reason: _,
            visible_top_row,
        } = change;
        self.render_diff_with(pane_render, pane_layout, size, attention_panes, &damage, |pane_id| {
            crate::benchmark_support::record_pane_snapshot();
            snapshots
                .get(&pane_id)
                .cloned()
                .map(|snapshot| crate::pty::PtyRenderSnapshot::benchmark(snapshot, visible_top_row))
                .ok_or_else(|| report!("muxr composer benchmark is missing a damaged pane snapshot"))
        })
    }

    #[cfg(feature = "benchmarking")]
    pub(crate) fn benchmark_full_diff(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        snapshots: &BTreeMap<PaneId, TerminalSnapshot>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        change: BenchmarkRenderChange,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let BenchmarkRenderChange {
            damage: _damage,
            reason,
            visible_top_row,
        } = change;
        let snapshots = benchmark_pane_snapshots(snapshots, pane_layout.active_pane, visible_top_row);
        let cached_layout = self.benchmark_pane_layout(pane_layout.pane_layout);
        let frame = Self::frame_from_snapshots(
            pane_render,
            pane_layout.active_pane,
            cached_layout,
            snapshots,
            size,
            attention_panes,
        )?;
        self.render_frame_diff(frame, reason)
    }

    #[cfg(feature = "benchmarking")]
    pub(crate) fn benchmark_end_to_end_diff(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        terminals: &BTreeMap<PaneId, TerminalState>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        change: BenchmarkRenderChange,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let BenchmarkRenderChange {
            damage,
            reason: _,
            visible_top_row,
        } = change;
        self.render_diff_with(pane_render, pane_layout, size, attention_panes, &damage, |pane_id| {
            let terminal = terminals
                .get(&pane_id)
                .ok_or_else(|| report!("muxr end-to-end benchmark is missing a damaged terminal"))?;
            PtyRenderSnapshot::benchmark_capture(terminal, Some(visible_top_row))
        })
    }

    #[cfg(feature = "benchmarking")]
    pub(crate) fn benchmark_end_to_end_full_diff(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        terminals: &BTreeMap<PaneId, TerminalState>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        change: BenchmarkRenderChange,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let BenchmarkRenderChange {
            damage: _damage,
            reason,
            visible_top_row,
        } = change;
        let snapshots = terminals
            .iter()
            .map(|(pane_id, terminal)| {
                PtyRenderSnapshot::benchmark_capture(
                    terminal,
                    (*pane_id == pane_layout.active_pane).then_some(visible_top_row),
                )
                .map(|snapshot| (*pane_id, snapshot))
            })
            .collect::<rootcause::Result<BTreeMap<_, _>>>()?;
        let cached_layout = self.benchmark_pane_layout(pane_layout.pane_layout);
        let frame = Self::frame_from_snapshots(
            pane_render,
            pane_layout.active_pane,
            cached_layout,
            snapshots,
            size,
            attention_panes,
        )?;
        self.render_frame_diff(frame, reason)
    }

    #[cfg(feature = "benchmarking")]
    pub(crate) fn benchmark_pane_regions(&self) -> rootcause::Result<Vec<BenchmarkPaneRegion>> {
        let frame = self
            .last_sent
            .as_ref()
            .ok_or_else(|| report!("muxr composer benchmark is missing its frame"))?;
        frame
            .pane_layout
            .regions()
            .iter()
            .map(|region| {
                frame
                    .pane_snapshots
                    .get(&region.id)
                    .map(|snapshot| BenchmarkPaneRegion {
                        mouse_mode: snapshot.mouse_mode(),
                        pane_id: region.id,
                        visible_row_wraps: snapshot.visible_row_wraps().to_vec(),
                        visible_top_row: snapshot.visible_top_row(),
                    })
                    .ok_or_else(|| report!("muxr composer benchmark is missing pane-region metadata"))
            })
            .collect()
    }

    #[cfg(feature = "benchmarking")]
    fn benchmark_pane_layout(&self, pane_layout: &PaneLayout) -> Arc<PaneLayout> {
        self.last_sent
            .as_ref()
            .filter(|frame| frame.pane_layout.as_ref() == pane_layout)
            .map_or_else(|| Arc::new(pane_layout.clone()), |frame| Arc::clone(&frame.pane_layout))
    }

    pub fn render_baseline(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
        attention_panes: &[PaneId],
    ) -> rootcause::Result<RenderUpdate> {
        self.render_frame_baseline(Self::current_frame(
            pane_render,
            pane_layout,
            runtimes,
            size,
            attention_panes,
        )?)
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

    pub fn render_diff(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        damage: &ClientRenderDmg,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        self.render_diff_with(pane_render, pane_layout, size, attention_panes, damage, |pane_id| {
            runtimes.pane_render_snapshot(pane_id)
        })
    }

    fn render_diff_with(
        &mut self,
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        damage: &ClientRenderDmg,
        mut snapshot: impl FnMut(PaneId) -> rootcause::Result<PtyRenderSnapshot>,
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
        mut snapshot: impl FnMut(PaneId) -> rootcause::Result<PtyRenderSnapshot>,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let use_cached_full = {
            let frame = self
                .last_sent
                .as_ref()
                .ok_or_else(|| report!("muxr partial render is missing its baseline"))?;
            AffectedRows::for_panes(&frame.pane_layout, pane_ids)?.spans_all(frame.size.rows())
        };
        if use_cached_full {
            return self.render_cached_full_diff_with(pane_ids, reason, snapshot);
        }
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
        mut snapshot: impl FnMut(PaneId) -> rootcause::Result<PtyRenderSnapshot>,
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
            pane_snapshots.insert(*pane_id, snapshot(*pane_id)?);
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

    pub fn pane_render_snapshot(&self, pane_id: PaneId) -> rootcause::Result<&PtyRenderSnapshot> {
        self.last_sent
            .as_ref()
            .and_then(|frame| frame.pane_snapshots.get(&pane_id))
            .ok_or_else(|| {
                report!("muxr composite frame is missing a pane snapshot").attach(format!("pane_id={pane_id}"))
            })
    }

    fn current_frame(
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
        attention_panes: &[PaneId],
    ) -> rootcause::Result<CompositeFrame> {
        Self::current_frame_with(pane_render, pane_layout, size, attention_panes, &mut |pane_id| {
            runtimes.pane_render_snapshot(pane_id)
        })
    }

    fn current_frame_with(
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        size: &TerminalSize,
        attention_panes: &[PaneId],
        snapshot: &mut impl FnMut(PaneId) -> rootcause::Result<PtyRenderSnapshot>,
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
        snapshot: &mut impl FnMut(PaneId) -> rootcause::Result<PtyRenderSnapshot>,
    ) -> rootcause::Result<CompositeFrame> {
        let mut pane_snapshots = BTreeMap::new();
        for region in pane_layout.regions() {
            pane_snapshots.insert(region.id, snapshot(region.id)?);
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
    ranges: SmallVec<[(u16, u16); 4]>,
}

impl AffectedRows {
    fn for_panes(pane_layout: &PaneLayout, pane_ids: &[PaneId]) -> rootcause::Result<Self> {
        let mut ranges = SmallVec::<[(u16, u16); 4]>::new();
        for pane_id in pane_ids {
            let region = pane_layout
                .regions()
                .iter()
                .find(|region| region.id == *pane_id)
                .ok_or_else(|| report!("muxr pane damage is outside the cached visible layout"))?;
            ranges.push((
                region.area.origin.row,
                region.area.origin.row.saturating_add(region.area.size.rows),
            ));
        }
        ranges.sort_by_key(|(start, _end)| *start);
        let mut merged = SmallVec::<[(u16, u16); 4]>::new();
        for (start, end) in ranges {
            if let Some((_previous_start, previous_end)) = merged.last_mut()
                && start <= *previous_end
            {
                *previous_end = (*previous_end).max(end);
            } else {
                merged.push((start, end));
            }
        }
        Ok(Self { ranges: merged })
    }

    fn contains(&self, row: u16) -> bool {
        self.ranges.iter().any(|(start, end)| row >= *start && row < *end)
    }

    fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.ranges.iter().flat_map(|(start, end)| *start..*end)
    }

    fn len(&self) -> usize {
        self.ranges
            .iter()
            .map(|(start, end)| usize::from(end.saturating_sub(*start)))
            .sum()
    }

    fn spans_all(&self, rows: u16) -> bool {
        self.ranges.as_slice() == [(0, rows)]
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
    snapshot: &mut impl FnMut(PaneId) -> rootcause::Result<PtyRenderSnapshot>,
) -> rootcause::Result<PartialFrameBefore> {
    let affected_rows = AffectedRows::for_panes(&frame.pane_layout, pane_ids)?;
    for pane_id in pane_ids {
        let snapshot = snapshot(*pane_id)?;
        frame.pane_snapshots.insert(*pane_id, snapshot);
    }
    let mut previous_rows = Vec::with_capacity(affected_rows.len());
    #[cfg(feature = "benchmarking")]
    crate::benchmark_support::record_rows_recomposed(affected_rows.len());
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
            Some(&previous.affected_rows),
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
    #[cfg(test)]
    verify_partial_frame(frame)?;
    Ok(previous)
}

#[cfg(feature = "benchmarking")]
fn benchmark_pane_snapshots(
    snapshots: &BTreeMap<PaneId, TerminalSnapshot>,
    changed_pane: PaneId,
    visible_top_row: u64,
) -> BTreeMap<PaneId, PtyRenderSnapshot> {
    snapshots
        .iter()
        .map(|(pane_id, snapshot)| {
            crate::benchmark_support::record_pane_snapshot();
            (
                *pane_id,
                PtyRenderSnapshot::benchmark(
                    snapshot.clone(),
                    if *pane_id == changed_pane { visible_top_row } else { 0 },
                ),
            )
        })
        .collect()
}

#[cfg(test)]
fn verify_partial_frame(frame: &CompositeFrame) -> rootcause::Result<()> {
    let oracle = RenderComposer::frame_from_snapshots(
        frame.pane_render,
        frame.active_pane,
        Arc::clone(&frame.pane_layout),
        frame.pane_snapshots.clone(),
        &frame.size,
        &frame.attention_panes,
    )?;
    if frame.rows != oracle.rows {
        let mismatch = frame
            .rows
            .iter()
            .zip(&oracle.rows)
            .enumerate()
            .find_map(|(row, (partial, full))| {
                partial
                    .iter()
                    .zip(full)
                    .enumerate()
                    .find_map(|(col, (partial, full))| (partial != full).then_some((row, col, partial, full)))
            });
        return Err(report!("muxr partial composer diverged from full composer").attach(format!("{mismatch:?}")));
    }
    if frame.cursor != oracle.cursor {
        return Err(report!("muxr partial composer cursor diverged from full composer"));
    }
    Ok(())
}

fn empty_render_rows(size: &TerminalSize) -> Vec<Vec<RenderCell>> {
    #[cfg(feature = "benchmarking")]
    {
        crate::benchmark_support::record_rows_initialized(usize::from(size.rows()));
        crate::benchmark_support::record_rows_recomposed(usize::from(size.rows()));
    }
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
    affected_rows: Option<&AffectedRows>,
) -> rootcause::Result<()> {
    if snapshot.size().cols() != region.area.size.cols || snapshot.size().rows() != region.area.size.rows {
        return Err(report!("muxr pane snapshot size does not match region")
            .attach(format!("pane_id={}", region.id))
            .attach(format!("snapshot_cols={}", snapshot.size().cols()))
            .attach(format!("snapshot_rows={}", snapshot.size().rows()))
            .attach(format!("region_cols={}", region.area.size.cols))
            .attach(format!("region_rows={}", region.area.size.rows)));
    }

    let mut url_links = crate::pane::url_links::detect_visible_url_links(snapshot.rows())?
        .into_iter()
        .peekable();
    for (span_index, span) in snapshot.rows().iter().enumerate() {
        let row = region
            .area
            .origin
            .row
            .checked_add(span.row())
            .ok_or_else(|| report!("muxr pane row offset overflowed"))?;
        if affected_rows.is_some_and(|affected| !affected.contains(row)) {
            while url_links.peek().is_some_and(|link| link.row() == span_index) {
                let _skipped = url_links.next();
            }
            continue;
        }
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
            #[cfg(feature = "benchmarking")]
            crate::benchmark_support::record_cells_copied(1);
        }
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

    #[cfg(feature = "benchmarking")]
    #[test]
    fn test_render_composer_benchmark_diff_when_region_change_hits_render_config_fallback_emits_empty_diff()
    -> rootcause::Result<()> {
        let pane_id = PaneId::new(1)?;
        let size = TerminalSize::new(2, 1)?;
        let pane_layout = PaneLayout::single_pane(pane_id, 1, &size);
        let mut terminal = crate::terminal::TerminalState::with_scrollback(&size, MuxrConfig::default().scrollback);
        let _ = terminal.process(b"ab");
        let snapshots = BTreeMap::from([(pane_id, terminal.snapshot()?)]);
        let config = MuxrConfig::default();
        let pane_render = PaneRenderConfig {
            border_styles: config.pane_borders,
            mode: BorderRenderMode::Focus,
            pane_attention: config.pane_attention,
            pane_dim: config.pane_dim,
        };
        let mut composer = RenderComposer::default();
        let _baseline = composer.benchmark_baseline(
            pane_render,
            PaneRenderLayout {
                active_pane: pane_id,
                pane_layout: &pane_layout,
            },
            &snapshots,
            &size,
            &[],
            0,
        )?;
        let mut resized_render = pane_render;
        resized_render.mode = BorderRenderMode::Resize;

        let update = composer.benchmark_diff(
            resized_render,
            PaneRenderLayout {
                active_pane: pane_id,
                pane_layout: &pane_layout,
            },
            &snapshots,
            &size,
            &[],
            BenchmarkRenderChange {
                damage: ClientRenderDmg::region_changed(pane_id),
                reason: RenderDiffReason::RegionChanged,
                visible_top_row: 0,
            },
        )?;

        let Some(RenderUpdate::Diff(diff)) = update else {
            return Err(report!("expected muxr region-change fallback diff"));
        };
        assert_that!(diff.base_seq(), eq(1));
        assert_that!(diff.seq(), eq(2));
        assert_that!(diff.rows(), points_to(empty()));
        Ok(())
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
}
