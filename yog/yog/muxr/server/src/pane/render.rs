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

use crate::pane::borders::BorderRenderMode;
use crate::pane::layout::PaneLayout;
use crate::pane::layout::PaneRegion;
use crate::pane::runtime::PaneRuntimes;
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
        let baseline = RenderBaseline::new(frame.seq, frame.size.clone(), frame.cursor.clone(), frame.rows.clone())?;
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
        reason: RenderDiffReason,
    ) -> rootcause::Result<Option<RenderUpdate>> {
        let Some(previous) = self.last_sent.as_ref() else {
            return Ok(Some(self.render_baseline(
                pane_render,
                pane_layout,
                runtimes,
                size,
                attention_panes,
            )?));
        };
        let frame = Self::current_frame(pane_render, pane_layout, runtimes, size, attention_panes)?;
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
        pane_render: PaneRenderConfig,
        pane_layout: PaneRenderLayout<'_>,
        runtimes: &PaneRuntimes,
        size: &TerminalSize,
        attention_panes: &[PaneId],
    ) -> rootcause::Result<CompositeFrame> {
        let mut rows = empty_render_rows(size);
        let mut cursor = RenderCursor {
            row: 0,
            col: 0,
            shape: muxr_core::RenderCursorShape::Default,
            visible: false,
        };

        for region in pane_layout.pane_layout.regions() {
            let snapshot = runtimes.snapshot(region.id)?;
            let visual_role = self::pane_visual_role(region.id, pane_layout.active_pane, attention_panes);
            paste_snapshot(
                &mut rows,
                region,
                &snapshot,
                visual_role.style(pane_render.pane_dim, pane_render.pane_attention),
            )?;
            if visual_role == PaneVisualRole::Normal && snapshot.cursor().visible {
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
                    shape: snapshot.cursor().shape,
                    visible: true,
                };
            }
        }
        crate::pane::borders::paste_borders(
            &mut rows,
            pane_render.border_styles,
            pane_render.pane_attention,
            pane_layout.pane_layout.borders(),
            Some(&pane_layout.active_pane),
            attention_panes,
            pane_render.mode,
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

fn pane_visual_role(pane_id: PaneId, active_pane: PaneId, attention_panes: &[PaneId]) -> PaneVisualRole {
    if pane_id == active_pane {
        return PaneVisualRole::Normal;
    }
    if attention_panes.contains(&pane_id) {
        return PaneVisualRole::Attention;
    }
    PaneVisualRole::Unfocused
}

fn paste_snapshot(
    rows: &mut [Vec<RenderCell>],
    region: &PaneRegion,
    snapshot: &TerminalSnapshot,
    visual_style: PaneVisualStyle,
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

    use super::*;
    use crate::pane::layout::PaneArea;
    use crate::pane::layout::PanePosition;
    use crate::pane::layout::PaneSize;

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
            shape: muxr_core::RenderCursorShape::Default,
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

        pretty_assertions::assert_eq!(
            self::pane_visual_role(PaneId::new(pane_id)?, PaneId::new(1)?, &attention_panes),
            expected
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

        pretty_assertions::assert_eq!(updated, style);
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

        assert2::assert!(updated.attrs.italic());
        assert2::assert!(!updated.attrs.dim());
        assert2::assert!(updated.bg != style.bg);
        assert2::assert!(updated.fg != style.fg);
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
        pretty_assertions::assert_eq!(linked_text, "https://example.com");
        for cell in linked_cells {
            assert2::assert!(cell.style().attrs.dim());
            pretty_assertions::assert_eq!(
                cell.hyperlink().map(muxr_core::RenderHyperlink::uri),
                Some("https://example.com")
            );
        }
        Ok(())
    }
}
