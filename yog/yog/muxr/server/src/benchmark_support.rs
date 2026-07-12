//! Feature-gated server performance seams for muxr development.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use muxr_config::MuxrConfig;
use muxr_core::PaneId;
use muxr_core::RenderUpdate;
use muxr_core::SessionName;
use muxr_core::TerminalSize;

use crate::pane::borders::BorderRenderMode;
use crate::pane::render::BenchmarkPaneRegion;
use crate::pane::render::BenchmarkRenderChange;
use crate::pane::render::PaneRenderConfig;
use crate::pane::render::PaneRenderLayout;
use crate::pane::render::RenderComposer;
use crate::pane::render::RenderDiffReason;
use crate::pane::split::PaneSplitAxis;
use crate::render_state::ClientRenderDmg;
use crate::state::SessionLayout;
use crate::state::SessionMetadata;
use crate::terminal::TerminalSnapshot;
use crate::terminal::TerminalState;

static BORDER_CELLS: AtomicU64 = AtomicU64::new(0);
static CELLS_COPIED: AtomicU64 = AtomicU64::new(0);
static PANE_SNAPSHOTS: AtomicU64 = AtomicU64::new(0);
static ROWS_INITIALIZED: AtomicU64 = AtomicU64::new(0);
static ROWS_RECOMPOSED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerDamage {
    Pane,
    RegionChanged,
    FullFallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerMatrix {
    ComposerOnly,
    EndToEnd,
}

impl ComposerMatrix {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ComposerOnly => "composer_only",
            Self::EndToEnd => "end_to_end",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ComposerCounters {
    pub border_cells: u64,
    pub cells_copied: u64,
    pub pane_snapshots: u64,
    pub rows_initialized: u64,
    pub rows_recomposed: u64,
}

pub fn reset_composer_counters() {
    BORDER_CELLS.store(0, Ordering::Relaxed);
    CELLS_COPIED.store(0, Ordering::Relaxed);
    PANE_SNAPSHOTS.store(0, Ordering::Relaxed);
    ROWS_INITIALIZED.store(0, Ordering::Relaxed);
    ROWS_RECOMPOSED.store(0, Ordering::Relaxed);
}

#[must_use]
pub fn composer_counters() -> ComposerCounters {
    ComposerCounters {
        border_cells: BORDER_CELLS.load(Ordering::Relaxed),
        cells_copied: CELLS_COPIED.load(Ordering::Relaxed),
        pane_snapshots: PANE_SNAPSHOTS.load(Ordering::Relaxed),
        rows_initialized: ROWS_INITIALIZED.load(Ordering::Relaxed),
        rows_recomposed: ROWS_RECOMPOSED.load(Ordering::Relaxed),
    }
}

pub(crate) fn record_border_cell() {
    let _previous = BORDER_CELLS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_cells_copied(count: usize) {
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    let _previous = CELLS_COPIED.fetch_add(count, Ordering::Relaxed);
}

pub(crate) fn record_pane_snapshot() {
    let _previous = PANE_SNAPSHOTS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_rows_initialized(count: usize) {
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    let _previous = ROWS_INITIALIZED.fetch_add(count, Ordering::Relaxed);
}

pub(crate) fn record_rows_recomposed(count: usize) {
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    let _previous = ROWS_RECOMPOSED.fetch_add(count, Ordering::Relaxed);
}

pub struct ComposerSample {
    pub counters: ComposerCounters,
    pub update: Option<RenderUpdate>,
}

pub struct ComposerWorkload {
    active_pane: PaneId,
    after: BTreeMap<PaneId, TerminalSnapshot>,
    after_terminals: BTreeMap<PaneId, TerminalState>,
    before: BTreeMap<PaneId, TerminalSnapshot>,
    before_terminals: BTreeMap<PaneId, TerminalState>,
    damage: ComposerDamage,
    layout: crate::pane::layout::PaneLayout,
    name: &'static str,
    size: TerminalSize,
}

pub struct ComposerRunner<'a> {
    composer: RenderComposer,
    current_after: bool,
    full: bool,
    matrix: ComposerMatrix,
    workload: &'a ComposerWorkload,
}

impl ComposerWorkload {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub fn runner(&self, matrix: ComposerMatrix) -> rootcause::Result<ComposerRunner<'_>> {
        self.runner_with_mode(matrix, false)
    }

    fn full_runner(&self, matrix: ComposerMatrix) -> rootcause::Result<ComposerRunner<'_>> {
        self.runner_with_mode(matrix, true)
    }

    fn runner_with_mode(&self, matrix: ComposerMatrix, full: bool) -> rootcause::Result<ComposerRunner<'_>> {
        let mut composer = RenderComposer::default();
        let _ = composer.benchmark_baseline(
            pane_render_config(BorderRenderMode::Focus),
            PaneRenderLayout {
                active_pane: self.active_pane,
                pane_layout: &self.layout,
            },
            &self.before,
            &self.size,
            &[],
            0,
        )?;
        Ok(ComposerRunner {
            composer,
            current_after: false,
            full,
            matrix,
            workload: self,
        })
    }
}

impl ComposerRunner<'_> {
    fn pane_regions(&self) -> rootcause::Result<Vec<BenchmarkPaneRegion>> {
        self.composer.benchmark_pane_regions()
    }

    pub fn step(&mut self) -> rootcause::Result<ComposerSample> {
        self.current_after = !self.current_after;
        let snapshots = if self.current_after {
            &self.workload.after
        } else {
            &self.workload.before
        };
        let terminals = if self.current_after {
            &self.workload.after_terminals
        } else {
            &self.workload.before_terminals
        };
        let reason = if self.workload.damage == ComposerDamage::RegionChanged {
            RenderDiffReason::RegionChanged
        } else {
            RenderDiffReason::DirtyFrame
        };
        let pane_layout = PaneRenderLayout {
            active_pane: self.workload.active_pane,
            pane_layout: &self.workload.layout,
        };
        let visible_top_row = u64::from(self.workload.damage == ComposerDamage::RegionChanged && self.current_after);
        // Alternating the border mode makes the fallback workload invalidate the cached render configuration.
        let pane_render = pane_render_config(
            if self.workload.damage == ComposerDamage::FullFallback && self.current_after {
                BorderRenderMode::Resize
            } else {
                BorderRenderMode::Focus
            },
        );
        let damage = match self.workload.damage {
            ComposerDamage::Pane | ComposerDamage::FullFallback => ClientRenderDmg::pane(self.workload.active_pane),
            ComposerDamage::RegionChanged => ClientRenderDmg::region_changed(self.workload.active_pane),
        };
        let change = BenchmarkRenderChange {
            damage,
            reason,
            visible_top_row,
        };
        let update = match (self.matrix, self.full) {
            (ComposerMatrix::ComposerOnly, true) => self.composer.benchmark_full_diff(
                pane_render,
                pane_layout,
                snapshots,
                &self.workload.size,
                &[],
                change,
            )?,
            (ComposerMatrix::ComposerOnly, false) => {
                self.composer
                    .benchmark_diff(pane_render, pane_layout, snapshots, &self.workload.size, &[], change)?
            }
            (ComposerMatrix::EndToEnd, true) => self.composer.benchmark_end_to_end_full_diff(
                pane_render,
                pane_layout,
                terminals,
                &self.workload.size,
                &[],
                change,
            )?,
            (ComposerMatrix::EndToEnd, false) => self.composer.benchmark_end_to_end_diff(
                pane_render,
                pane_layout,
                terminals,
                &self.workload.size,
                &[],
                change,
            )?,
        };
        Ok(ComposerSample {
            counters: self::composer_counters(),
            update,
        })
    }
}

pub fn composer_workloads() -> rootcause::Result<Vec<ComposerWorkload>> {
    Ok(vec![
        workload(
            "server_composer/two_pane_interactive",
            2,
            80,
            24,
            ComposerDamage::Pane,
            b"interactive",
        )?,
        workload(
            "server_composer/four_pane_burst",
            4,
            160,
            48,
            ComposerDamage::Pane,
            &vec![b'x'; 8192],
        )?,
        workload(
            "server_composer/one_dirty_pane",
            4,
            160,
            48,
            ComposerDamage::Pane,
            b"dirty",
        )?,
        workload(
            "server_composer/region_changed",
            2,
            80,
            24,
            ComposerDamage::RegionChanged,
            b"muxr baseline",
        )?,
        workload(
            "server_composer/full_fallback",
            4,
            160,
            48,
            ComposerDamage::FullFallback,
            b"fallback",
        )?,
    ])
}

pub fn verify_composer_oracle() -> rootcause::Result<()> {
    for workload in composer_workloads()? {
        for matrix in [ComposerMatrix::ComposerOnly, ComposerMatrix::EndToEnd] {
            let mut expected = workload.full_runner(matrix)?;
            let mut actual = workload.runner(matrix)?;
            for _ in 0..3 {
                let expected_sample = expected.step()?;
                let actual_sample = actual.step()?;
                if expected_sample.update != actual_sample.update
                    || expected.pane_regions()? != actual.pane_regions()?
                {
                    return Err(rootcause::report!("muxr server composer benchmark oracle mismatch")
                        .attach(format!("matrix={}", matrix.name()))
                        .attach(format!("workload={}", workload.name)));
                }
            }
        }
    }
    Ok(())
}

fn workload(
    name: &'static str,
    panes: usize,
    cols: u16,
    rows: u16,
    damage: ComposerDamage,
    changed: &[u8],
) -> rootcause::Result<ComposerWorkload> {
    let size = TerminalSize::new(cols, rows)?;
    let session: SessionName = "benchmark".parse()?;
    let mut session_layout = SessionLayout::initial(&session, metadata(1))?;
    for pane in 1..panes {
        let axis = if pane % 2 == 0 {
            PaneSplitAxis::Horizontal
        } else {
            PaneSplitAxis::Vertical
        };
        let _ =
            session_layout.split_active_pane(MuxrConfig::default().layout, metadata(pane.saturating_add(1)), axis)?;
    }
    let layout = session_layout.pane_layout(&size)?;
    let active_pane = session_layout.active_pane_id()?;
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    let mut before_terminals = BTreeMap::new();
    let mut after_terminals = BTreeMap::new();
    for region in layout.regions() {
        let pane_size = TerminalSize::new(region.area.size.cols, region.area.size.rows)?;
        let before_terminal = terminal_state(&pane_size, b"muxr baseline");
        let bytes = if region.id == active_pane {
            changed
        } else {
            b"muxr baseline"
        };
        let after_terminal = terminal_state(&pane_size, bytes);
        before.insert(region.id, before_terminal.snapshot()?);
        after.insert(region.id, after_terminal.snapshot()?);
        before_terminals.insert(region.id, before_terminal);
        after_terminals.insert(region.id, after_terminal);
    }
    Ok(ComposerWorkload {
        active_pane,
        after,
        after_terminals,
        before,
        before_terminals,
        damage,
        layout,
        name,
        size,
    })
}

fn metadata(started_at: usize) -> SessionMetadata {
    SessionMetadata {
        cmd_label: "benchmark".to_owned(),
        cwd: "/tmp".to_owned(),
        started_at: u64::try_from(started_at).unwrap_or(u64::MAX),
    }
}

fn terminal_state(size: &TerminalSize, bytes: &[u8]) -> TerminalState {
    let mut terminal = TerminalState::with_scrollback(size, MuxrConfig::default().scrollback);
    let _ = terminal.process(bytes);
    terminal
}

fn pane_render_config(mode: BorderRenderMode) -> PaneRenderConfig {
    let config = MuxrConfig::default();
    PaneRenderConfig {
        border_styles: config.pane_borders,
        mode,
        pane_attention: config.pane_attention,
        pane_dim: config.pane_dim,
    }
}

/// Observe the current benchmark process through the production OS lookup path.
#[must_use]
pub fn observe_current_process() -> (bool, bool) {
    crate::pane::cmd::benchmark_current_process_observation()
}
