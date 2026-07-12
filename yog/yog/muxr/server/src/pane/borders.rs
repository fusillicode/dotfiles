use std::collections::BTreeMap;

use muxr_config::CellStyle;
use muxr_config::PaneAttentionConfig;
use muxr_config::PaneBorderStyles;
use muxr_core::PaneId;
use muxr_core::RenderCell;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use rootcause::report;

use crate::pane::layout::PanePosition;
use crate::pane::layout::PaneRegion;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneBorder {
    axis: PaneBorderAxis,
    len: u16,
    owner_cells: Vec<PaneBorderCellOwners>,
    position: PanePosition,
}

impl PaneBorder {
    pub fn with_adjacent_regions(
        axis: PaneBorderAxis,
        position: PanePosition,
        len: u16,
        first_regions: &[PaneRegion],
        second_regions: &[PaneRegion],
    ) -> rootcause::Result<Self> {
        let mut owner_cells = Vec::new();
        for offset in 0..len {
            let cell_position = match axis {
                PaneBorderAxis::Horizontal => PanePosition {
                    row: position.row,
                    col: position
                        .col
                        .checked_add(offset)
                        .ok_or_else(|| report!("muxr horizontal pane border owner col overflowed"))?,
                },
                PaneBorderAxis::Vertical => PanePosition {
                    row: position
                        .row
                        .checked_add(offset)
                        .ok_or_else(|| report!("muxr vertical pane border owner row overflowed"))?,
                    col: position.col,
                },
            };
            let pane_ids = first_regions
                .iter()
                .chain(second_regions)
                .filter(|region| {
                    BorderCellOwner::from_region_border_cell(region, axis, cell_position) == BorderCellOwner::Owned
                })
                .map(|region| region.id)
                .collect::<Vec<_>>();
            if !pane_ids.is_empty() {
                owner_cells.push(PaneBorderCellOwners { offset, pane_ids });
            }
        }

        Ok(Self {
            axis,
            len,
            owner_cells,
            position,
        })
    }

    pub const fn axis(&self) -> PaneBorderAxis {
        self.axis
    }

    pub const fn col(&self) -> u16 {
        self.position.col
    }

    pub const fn len(&self) -> u16 {
        self.len
    }

    pub const fn row(&self) -> u16 {
        self.position.row
    }

    pub fn ownership(&self, position: PanePosition, pane_id: PaneId) -> BorderCellOwner {
        let Some(offset) = self.cell_offset(position) else {
            return BorderCellOwner::Unowned;
        };

        if self
            .owner_cells
            .iter()
            .any(|cell| cell.offset == offset && cell.pane_ids.contains(&pane_id))
        {
            BorderCellOwner::Owned
        } else {
            BorderCellOwner::Unowned
        }
    }

    fn cell_offset(&self, position: PanePosition) -> Option<u16> {
        match self.axis {
            PaneBorderAxis::Horizontal if position.row == self.position.row => {
                let offset = position.col.checked_sub(self.position.col)?;
                (offset < self.len).then_some(offset)
            }
            PaneBorderAxis::Vertical if position.col == self.position.col => {
                let offset = position.row.checked_sub(self.position.row)?;
                (offset < self.len).then_some(offset)
            }
            PaneBorderAxis::Horizontal | PaneBorderAxis::Vertical => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PaneBorderCellOwners {
    offset: u16,
    pane_ids: Vec<PaneId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneBorderAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BorderRenderMode {
    Focus,
    Resize,
}

#[derive(Clone, Copy)]
pub struct PasteBordersConfig<'a> {
    pub active_pane: Option<&'a PaneId>,
    pub attention_panes: &'a [PaneId],
    pub border_mode: BorderRenderMode,
    pub borders: &'a [PaneBorder],
    pub pane_attention: PaneAttentionConfig,
    pub styles: PaneBorderStyles,
}

pub fn paste_borders(
    rows: &mut [Vec<RenderCell>],
    styles: PaneBorderStyles,
    pane_attention: PaneAttentionConfig,
    borders: &[PaneBorder],
    active_pane: Option<&PaneId>,
    attention_panes: &[PaneId],
    border_mode: BorderRenderMode,
) -> rootcause::Result<()> {
    self::paste_borders_in_rows(
        rows,
        PasteBordersConfig {
            active_pane,
            attention_panes,
            border_mode,
            borders,
            pane_attention,
            styles,
        },
        |_| true,
    )
}

pub fn paste_borders_in_rows(
    rows: &mut [Vec<RenderCell>],
    config: PasteBordersConfig<'_>,
    include_row: impl Fn(u16) -> bool,
) -> rootcause::Result<()> {
    let border_cells = self::compose_border_cells(
        config.borders,
        config.active_pane,
        config.attention_panes,
        config.border_mode,
    )?;
    for ((row, col), cell) in border_cells {
        if !include_row(row) {
            continue;
        }
        #[cfg(feature = "benchmarking")]
        crate::benchmark_support::record_border_cell();
        let target_row = rows
            .get_mut(usize::from(row))
            .ok_or_else(|| report!("muxr pane border row outside composite frame"))?;
        let target = target_row
            .get_mut(usize::from(col))
            .ok_or_else(|| report!("muxr pane border col outside composite frame"))?;
        *target = RenderCell::narrow(
            cell.shape.glyph(),
            cell.visual.style(config.styles, config.pane_attention),
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BorderCellOwner {
    Owned,
    Unowned,
}

impl BorderCellOwner {
    fn from_region_border_cell(region: &PaneRegion, axis: PaneBorderAxis, position: PanePosition) -> Self {
        match axis {
            PaneBorderAxis::Horizontal => {
                // Pane regions exclude separator cells; extending the perpendicular span by exactly one cell
                // gives split junctions to the pane corner without coloring unrelated diagonal border cells.
                let col = u32::from(position.col);
                let start_col = u32::from(region.area.origin.col).saturating_sub(1);
                let end_col = region.area.end_col_exclusive();
                let contains_col = col >= start_col && col <= end_col;
                let touches_top = position.row.checked_add(1) == Some(region.area.origin.row) && contains_col;
                let touches_bottom = region.area.end_row() == Some(position.row) && contains_col;
                if touches_top || touches_bottom {
                    Self::Owned
                } else {
                    Self::Unowned
                }
            }
            PaneBorderAxis::Vertical => {
                let row = u32::from(position.row);
                let start_row = u32::from(region.area.origin.row).saturating_sub(1);
                let end_row = region.area.end_row_exclusive();
                let contains_row = row >= start_row && row <= end_row;
                let touches_left = position.col.checked_add(1) == Some(region.area.origin.col) && contains_row;
                let touches_right = region.area.end_col() == Some(position.col) && contains_row;
                if touches_left || touches_right {
                    Self::Owned
                } else {
                    Self::Unowned
                }
            }
        }
    }
}

fn compose_border_cells(
    borders: &[PaneBorder],
    active_pane: Option<&PaneId>,
    attention_panes: &[PaneId],
    border_mode: BorderRenderMode,
) -> rootcause::Result<BTreeMap<(u16, u16), BorderCell>> {
    let mut cells = BTreeMap::new();
    for border in borders {
        match border.axis() {
            PaneBorderAxis::Horizontal => {
                for offset in 0..border.len() {
                    let col = border
                        .col()
                        .checked_add(offset)
                        .ok_or_else(|| report!("muxr horizontal pane border col overflowed"))?;
                    let visual = self::border_visual_for_cell(
                        border,
                        border.row(),
                        col,
                        active_pane,
                        attention_panes,
                        border_mode,
                    );
                    self::add_border_cell(&mut cells, border.row(), col, border.axis(), visual);
                }
            }
            PaneBorderAxis::Vertical => {
                let end_row = border
                    .row()
                    .checked_add(border.len())
                    .ok_or_else(|| report!("muxr vertical pane border end overflowed"))?;
                for row in border.row()..end_row {
                    let visual = self::border_visual_for_cell(
                        border,
                        row,
                        border.col(),
                        active_pane,
                        attention_panes,
                        border_mode,
                    );
                    self::add_border_cell(&mut cells, row, border.col(), border.axis(), visual);
                }
            }
        }
    }
    self::add_adjacent_border_junctions(&mut cells);
    Ok(cells)
}

fn add_border_cell(
    cells: &mut BTreeMap<(u16, u16), BorderCell>,
    row: u16,
    col: u16,
    axis: PaneBorderAxis,
    visual: BorderVisual,
) {
    cells
        .entry((row, col))
        .and_modify(|cell| {
            cell.shape = cell.shape.with_axis(axis);
            cell.visual = cell.visual.stronger(visual);
        })
        .or_insert_with(|| BorderCell {
            shape: BorderCellShape::from_axis(axis),
            visual,
        });
}

fn add_adjacent_border_junctions(cells: &mut BTreeMap<(u16, u16), BorderCell>) {
    // Nested splits place child borders next to parent borders, not always on the same cell; compose those
    // neighbor edges so the rendered frame uses connected junction glyphs instead of detached line segments.
    let base_cells = cells.clone();
    for ((row, col), cell) in &base_cells {
        if cell.shape.horizontal_state() == BorderAxisEdgeState::Present {
            if let Some(left_col) = col.checked_sub(1)
                && base_cells
                    .get(&(*row, left_col))
                    .is_some_and(|neighbor| neighbor.shape.vertical_state() == BorderAxisEdgeState::Present)
            {
                self::merge_border_edge(cells, (*row, left_col), BorderCellEdge::Right, cell.visual);
            }
            if let Some(right_col) = col.checked_add(1)
                && base_cells
                    .get(&(*row, right_col))
                    .is_some_and(|neighbor| neighbor.shape.vertical_state() == BorderAxisEdgeState::Present)
            {
                self::merge_border_edge(cells, (*row, right_col), BorderCellEdge::Left, cell.visual);
            }
        }
        if cell.shape.vertical_state() == BorderAxisEdgeState::Present {
            if let Some(up_row) = row.checked_sub(1)
                && base_cells
                    .get(&(up_row, *col))
                    .is_some_and(|neighbor| neighbor.shape.horizontal_state() == BorderAxisEdgeState::Present)
            {
                self::merge_border_edge(cells, (up_row, *col), BorderCellEdge::Down, cell.visual);
            }
            if let Some(down_row) = row.checked_add(1)
                && base_cells
                    .get(&(down_row, *col))
                    .is_some_and(|neighbor| neighbor.shape.horizontal_state() == BorderAxisEdgeState::Present)
            {
                self::merge_border_edge(cells, (down_row, *col), BorderCellEdge::Up, cell.visual);
            }
        }
    }
}

fn merge_border_edge(
    cells: &mut BTreeMap<(u16, u16), BorderCell>,
    position: (u16, u16),
    edge: BorderCellEdge,
    visual: BorderVisual,
) {
    if let Some(cell) = cells.get_mut(&position) {
        cell.shape = cell.shape.with_edge(edge);
        cell.visual = cell.visual.stronger(visual);
    }
}

fn border_visual_for_cell(
    border: &PaneBorder,
    row: u16,
    col: u16,
    active_pane: Option<&PaneId>,
    attention_panes: &[PaneId],
    border_mode: BorderRenderMode,
) -> BorderVisual {
    let position = PanePosition { row, col };
    let owned_by_active_pane =
        active_pane.is_some_and(|pane_id| border.ownership(position, *pane_id) == BorderCellOwner::Owned);
    let owned_by_attention_pane = attention_panes
        .iter()
        .any(|pane_id| active_pane != Some(pane_id) && border.ownership(position, *pane_id) == BorderCellOwner::Owned);

    if owned_by_active_pane {
        return match border_mode {
            BorderRenderMode::Focus if owned_by_attention_pane => BorderVisual::Attention,
            BorderRenderMode::Focus => BorderVisual::Focused,
            BorderRenderMode::Resize => BorderVisual::Resize,
        };
    }

    if owned_by_attention_pane {
        return BorderVisual::Attention;
    }

    BorderVisual::Default
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorderCellEdge {
    Down,
    Left,
    Right,
    Up,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorderCellShape {
    All,
    Down,
    DownLeft,
    DownLeftRight,
    DownRight,
    Empty,
    Horizontal,
    Left,
    Right,
    Up,
    UpDownLeft,
    UpDownRight,
    UpLeft,
    UpLeftRight,
    UpRight,
    Vertical,
}

impl BorderCellShape {
    const fn from_axis(axis: PaneBorderAxis) -> Self {
        match axis {
            PaneBorderAxis::Horizontal => Self::Horizontal,
            PaneBorderAxis::Vertical => Self::Vertical,
        }
    }

    const fn with_axis(self, axis: PaneBorderAxis) -> Self {
        match axis {
            PaneBorderAxis::Horizontal => Self::from_edges((
                self.up(),
                self.down(),
                BorderEdgeState::Present,
                BorderEdgeState::Present,
            )),
            PaneBorderAxis::Vertical => Self::from_edges((
                BorderEdgeState::Present,
                BorderEdgeState::Present,
                self.left(),
                self.right(),
            )),
        }
    }

    const fn with_edge(self, edge: BorderCellEdge) -> Self {
        match edge {
            BorderCellEdge::Down => Self::from_edges((self.up(), BorderEdgeState::Present, self.left(), self.right())),
            BorderCellEdge::Left => Self::from_edges((self.up(), self.down(), BorderEdgeState::Present, self.right())),
            BorderCellEdge::Right => Self::from_edges((self.up(), self.down(), self.left(), BorderEdgeState::Present)),
            BorderCellEdge::Up => Self::from_edges((BorderEdgeState::Present, self.down(), self.left(), self.right())),
        }
    }

    const fn horizontal_state(self) -> BorderAxisEdgeState {
        match self.left() {
            BorderEdgeState::Present => BorderAxisEdgeState::Present,
            BorderEdgeState::Absent => match self.right() {
                BorderEdgeState::Present => BorderAxisEdgeState::Present,
                BorderEdgeState::Absent => BorderAxisEdgeState::Absent,
            },
        }
    }

    const fn vertical_state(self) -> BorderAxisEdgeState {
        match self.up() {
            BorderEdgeState::Present => BorderAxisEdgeState::Present,
            BorderEdgeState::Absent => match self.down() {
                BorderEdgeState::Present => BorderAxisEdgeState::Present,
                BorderEdgeState::Absent => BorderAxisEdgeState::Absent,
            },
        }
    }

    const fn glyph(self) -> &'static str {
        match self {
            Self::All => "┼",
            Self::UpDownLeft => "┤",
            Self::UpDownRight => "├",
            Self::Vertical | Self::Up | Self::Down => "│",
            Self::UpLeftRight => "┴",
            Self::DownLeftRight => "┬",
            Self::Horizontal | Self::Left | Self::Right => "─",
            Self::UpRight => "└",
            Self::UpLeft => "┘",
            Self::DownRight => "┌",
            Self::DownLeft => "┐",
            Self::Empty => " ",
        }
    }

    const fn from_edges(edges: (BorderEdgeState, BorderEdgeState, BorderEdgeState, BorderEdgeState)) -> Self {
        use BorderEdgeState::Absent as A;
        use BorderEdgeState::Present as P;
        match edges {
            (A, A, A, A) => Self::Empty,
            (P, A, A, A) => Self::Up,
            (A, P, A, A) => Self::Down,
            (A, A, P, A) => Self::Left,
            (A, A, A, P) => Self::Right,
            (P, P, A, A) => Self::Vertical,
            (A, A, P, P) => Self::Horizontal,
            (P, A, P, A) => Self::UpLeft,
            (P, A, A, P) => Self::UpRight,
            (A, P, P, A) => Self::DownLeft,
            (A, P, A, P) => Self::DownRight,
            (P, P, P, A) => Self::UpDownLeft,
            (P, P, A, P) => Self::UpDownRight,
            (P, A, P, P) => Self::UpLeftRight,
            (A, P, P, P) => Self::DownLeftRight,
            (P, P, P, P) => Self::All,
        }
    }

    const fn up(self) -> BorderEdgeState {
        match self {
            Self::All
            | Self::Up
            | Self::UpDownLeft
            | Self::UpDownRight
            | Self::UpLeft
            | Self::UpLeftRight
            | Self::UpRight
            | Self::Vertical => BorderEdgeState::Present,
            Self::Down
            | Self::DownLeft
            | Self::DownLeftRight
            | Self::DownRight
            | Self::Empty
            | Self::Horizontal
            | Self::Left
            | Self::Right => BorderEdgeState::Absent,
        }
    }

    const fn down(self) -> BorderEdgeState {
        match self {
            Self::All
            | Self::Down
            | Self::DownLeft
            | Self::DownLeftRight
            | Self::DownRight
            | Self::UpDownLeft
            | Self::UpDownRight
            | Self::Vertical => BorderEdgeState::Present,
            Self::Empty
            | Self::Horizontal
            | Self::Left
            | Self::Right
            | Self::Up
            | Self::UpLeft
            | Self::UpLeftRight
            | Self::UpRight => BorderEdgeState::Absent,
        }
    }

    const fn left(self) -> BorderEdgeState {
        match self {
            Self::All
            | Self::DownLeft
            | Self::DownLeftRight
            | Self::Horizontal
            | Self::Left
            | Self::UpDownLeft
            | Self::UpLeft
            | Self::UpLeftRight => BorderEdgeState::Present,
            Self::Down
            | Self::DownRight
            | Self::Empty
            | Self::Right
            | Self::Up
            | Self::UpDownRight
            | Self::UpRight
            | Self::Vertical => BorderEdgeState::Absent,
        }
    }

    const fn right(self) -> BorderEdgeState {
        match self {
            Self::All
            | Self::DownLeftRight
            | Self::DownRight
            | Self::Horizontal
            | Self::Right
            | Self::UpDownRight
            | Self::UpLeftRight
            | Self::UpRight => BorderEdgeState::Present,
            Self::Down
            | Self::DownLeft
            | Self::Empty
            | Self::Left
            | Self::Up
            | Self::UpDownLeft
            | Self::UpLeft
            | Self::Vertical => BorderEdgeState::Absent,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorderEdgeState {
    Absent,
    Present,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorderAxisEdgeState {
    Absent,
    Present,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BorderCell {
    shape: BorderCellShape,
    visual: BorderVisual,
}

// Pane borders can use exact RGB for pane state contrast; indexed colors remain useful for palette-sized accents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorderVisual {
    Default,
    Focused,
    Attention,
    Resize,
}

impl BorderVisual {
    const fn stronger(self, incoming: Self) -> Self {
        if incoming.priority() > self.priority() {
            incoming
        } else {
            self
        }
    }

    const fn style(self, styles: PaneBorderStyles, pane_attention: PaneAttentionConfig) -> RenderStyle {
        match self {
            Self::Default => self::render_style(styles.default),
            Self::Focused => self::render_style(styles.focused),
            Self::Attention => self::render_style(pane_attention.border),
            Self::Resize => self::render_style(styles.resize),
        }
    }

    const fn priority(self) -> u8 {
        match self {
            Self::Default => 0,
            Self::Focused => 1,
            Self::Attention => 2,
            Self::Resize => 3,
        }
    }
}

const fn render_style(style: CellStyle) -> RenderStyle {
    RenderStyle {
        attrs: RenderTextStyle::empty().set_bold(style.attrs.bold),
        bg: style.bg,
        fg: style.fg,
    }
}

#[cfg(test)]
mod tests {
    use muxr_config::MuxrConfig;
    use muxr_core::PaneId;
    use muxr_core::TerminalSize;
    use rootcause::report;
    use test_that::prelude::*;

    use super::*;
    use crate::pane::layout::PaneArea;
    use crate::pane::layout::PanePosition;
    use crate::pane::layout::PaneRegion;
    use crate::pane::layout::PaneSize;

    fn pane_region(id: PaneId, row: u16, col: u16, rows: u16, cols: u16, focus_seq: u64) -> PaneRegion {
        PaneRegion {
            area: PaneArea {
                origin: PanePosition { row, col },
                size: PaneSize { rows, cols },
            },
            focus_seq,
            id,
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum NestedBorderFixture {
        HorizontalThenVertical,
        VerticalThenHorizontal,
    }

    impl NestedBorderFixture {
        fn borders(self) -> rootcause::Result<Vec<PaneBorder>> {
            let vertical_left = self::pane_region(PaneId::new(1)?, 0, 0, 24, 40, 1);
            let vertical_top_right = self::pane_region(PaneId::new(2)?, 0, 41, 12, 39, 2);
            let vertical_bottom_right = self::pane_region(PaneId::new(3)?, 13, 41, 11, 39, 3);
            let horizontal_top = self::pane_region(PaneId::new(1)?, 0, 0, 12, 80, 1);
            let horizontal_bottom_left = self::pane_region(PaneId::new(2)?, 13, 0, 11, 40, 2);
            let horizontal_bottom_right = self::pane_region(PaneId::new(3)?, 13, 41, 11, 39, 3);

            match self {
                Self::VerticalThenHorizontal => Ok(vec![
                    PaneBorder::with_adjacent_regions(
                        PaneBorderAxis::Vertical,
                        PanePosition { row: 0, col: 40 },
                        24,
                        std::slice::from_ref(&vertical_left),
                        &[vertical_top_right.clone(), vertical_bottom_right.clone()],
                    )?,
                    PaneBorder::with_adjacent_regions(
                        PaneBorderAxis::Horizontal,
                        PanePosition { row: 12, col: 41 },
                        39,
                        std::slice::from_ref(&vertical_top_right),
                        std::slice::from_ref(&vertical_bottom_right),
                    )?,
                ]),
                Self::HorizontalThenVertical => Ok(vec![
                    PaneBorder::with_adjacent_regions(
                        PaneBorderAxis::Horizontal,
                        PanePosition { row: 12, col: 0 },
                        80,
                        std::slice::from_ref(&horizontal_top),
                        &[horizontal_bottom_left.clone(), horizontal_bottom_right.clone()],
                    )?,
                    PaneBorder::with_adjacent_regions(
                        PaneBorderAxis::Vertical,
                        PanePosition { row: 13, col: 40 },
                        11,
                        std::slice::from_ref(&horizontal_bottom_left),
                        std::slice::from_ref(&horizontal_bottom_right),
                    )?,
                ]),
            }
        }
    }

    #[rstest::rstest]
    #[case::vertical_corner(PaneBorderAxis::Vertical, 1, 0, 3, 1, 1)]
    #[case::horizontal_corner(PaneBorderAxis::Horizontal, 0, 1, 3, 1, 1)]
    fn test_pane_border_with_adjacent_regions_when_cell_is_pane_corner_records_owner(
        #[case] axis: PaneBorderAxis,
        #[case] border_col: u16,
        #[case] border_row: u16,
        #[case] border_len: u16,
        #[case] cell_col: u16,
        #[case] cell_row: u16,
    ) -> rootcause::Result<()> {
        let active_pane = PaneId::new(1)?;
        let active_region = self::pane_region(active_pane, 2, 2, 1, 1, 1);
        let border = PaneBorder::with_adjacent_regions(
            axis,
            PanePosition {
                row: border_row,
                col: border_col,
            },
            border_len,
            &[],
            std::slice::from_ref(&active_region),
        )?;

        assert_that!(
            border.ownership(
                PanePosition {
                    row: cell_row,
                    col: cell_col
                },
                active_pane
            ),
            eq(BorderCellOwner::Owned)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical_two_rows_above(PaneBorderAxis::Vertical, 1, 0, 3, 1, 0)]
    #[case::horizontal_two_cols_before(PaneBorderAxis::Horizontal, 0, 1, 3, 0, 1)]
    fn test_pane_border_with_adjacent_regions_when_cell_is_unrelated_diagonal_does_not_record_owner(
        #[case] axis: PaneBorderAxis,
        #[case] border_col: u16,
        #[case] border_row: u16,
        #[case] border_len: u16,
        #[case] cell_col: u16,
        #[case] cell_row: u16,
    ) -> rootcause::Result<()> {
        let active_pane = PaneId::new(1)?;
        let active_region = self::pane_region(active_pane, 2, 2, 1, 1, 1);
        let border = PaneBorder::with_adjacent_regions(
            axis,
            PanePosition {
                row: border_row,
                col: border_col,
            },
            border_len,
            &[],
            std::slice::from_ref(&active_region),
        )?;

        assert_that!(
            border.ownership(
                PanePosition {
                    row: cell_row,
                    col: cell_col
                },
                active_pane
            ),
            eq(BorderCellOwner::Unowned)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical_then_horizontal_focus_lower_right(
        NestedBorderFixture::VerticalThenHorizontal,
        3,
        vec![(12, 40, "├"), (13, 40, "│"), (12, 41, "─")],
    )]
    #[case::vertical_then_horizontal_focus_upper_right(
        NestedBorderFixture::VerticalThenHorizontal,
        2,
        vec![(0, 40, "│"), (11, 40, "│"), (12, 40, "├"), (12, 41, "─")],
    )]
    #[case::horizontal_then_vertical_focus_lower_right(
        NestedBorderFixture::HorizontalThenVertical,
        3,
        vec![(12, 40, "┬"), (12, 41, "─"), (13, 40, "│")],
    )]
    #[case::horizontal_then_vertical_focus_lower_left(
        NestedBorderFixture::HorizontalThenVertical,
        2,
        vec![(12, 39, "─"), (12, 40, "┬"), (13, 40, "│")],
    )]
    fn test_paste_borders_when_active_pane_touches_parent_and_child_borders_highlights_outline(
        #[case] fixture: NestedBorderFixture,
        #[case] active_pane: u32,
        #[case] expected_focused_cells: Vec<(u16, u16, &'static str)>,
    ) -> rootcause::Result<()> {
        let active_pane = PaneId::new(active_pane)?;
        let size = TerminalSize::new(80, 24)?;
        let borders = fixture.borders()?;
        let mut rows = self::empty_render_rows(&size);
        self::paste_borders(
            &mut rows,
            MuxrConfig::default().pane_borders,
            MuxrConfig::default().pane_attention,
            &borders,
            Some(&active_pane),
            &[],
            BorderRenderMode::Focus,
        )?;
        let border_cells = self::compose_border_cells(&borders, Some(&active_pane), &[], BorderRenderMode::Focus)?;

        for (row, col, glyph) in expected_focused_cells {
            let cell = rows
                .get(usize::from(row))
                .and_then(|row| row.get(usize::from(col)))
                .ok_or_else(|| report!("expected focused border cell").attach(format!("row={row} col={col}")))?;
            assert_that!(cell.text(), eq(glyph));
            assert_that!(
                self::border_cell_at(&border_cells, row, col)?.visual,
                eq(BorderVisual::Focused)
            );
        }
        Ok(())
    }

    #[test]
    fn test_paste_borders_when_borders_are_rendered_uses_box_drawing_style() -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let borders = vec![
            PaneBorder::with_adjacent_regions(PaneBorderAxis::Vertical, PanePosition { row: 0, col: 1 }, 3, &[], &[])?,
            PaneBorder::with_adjacent_regions(
                PaneBorderAxis::Horizontal,
                PanePosition { row: 1, col: 0 },
                3,
                &[],
                &[],
            )?,
        ];

        self::paste_borders(
            &mut rows,
            MuxrConfig::default().pane_borders,
            MuxrConfig::default().pane_attention,
            &borders,
            None,
            &[],
            BorderRenderMode::Focus,
        )?;
        let border_cells = self::compose_border_cells(&borders, None, &[], BorderRenderMode::Focus)?;

        let vertical_cell = rows
            .first()
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected vertical border cell"))?;
        let horizontal_cell = rows
            .get(1)
            .and_then(|row| row.first())
            .ok_or_else(|| report!("expected horizontal border cell"))?;
        let junction_cell = rows
            .get(1)
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected junction border cell"))?;

        assert_that!(vertical_cell.text(), eq("│"));
        assert_that!(horizontal_cell.text(), eq("─"));
        assert_that!(junction_cell.text(), eq("┼"));
        assert_that!(
            self::border_cell_at(&border_cells, 0, 1)?.visual,
            eq(BorderVisual::Default)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::focus(BorderRenderMode::Focus, BorderVisual::Focused)]
    #[case::resize(BorderRenderMode::Resize, BorderVisual::Resize)]
    fn test_paste_borders_when_border_cell_is_owned_by_active_pane_uses_mode_style(
        #[case] mode: BorderRenderMode,
        #[case] expected_visual: BorderVisual,
    ) -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let active_pane = PaneId::new(1)?;
        let active_region = self::pane_region(active_pane, 0, 0, 3, 1, 1);
        let border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Vertical,
            PanePosition { row: 0, col: 1 },
            3,
            std::slice::from_ref(&active_region),
            &[],
        )?;

        let border_cells = self::compose_border_cells(std::slice::from_ref(&border), Some(&active_pane), &[], mode)?;
        self::paste_borders(
            &mut rows,
            MuxrConfig::default().pane_borders,
            MuxrConfig::default().pane_attention,
            &[border],
            Some(&active_pane),
            &[],
            mode,
        )?;

        let border_cell = rows
            .first()
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected active border cell"))?;

        assert_that!(border_cell.text(), eq("│"));
        assert_that!(self::border_cell_at(&border_cells, 0, 1)?.visual, eq(expected_visual));
        Ok(())
    }

    #[test]
    fn test_paste_borders_when_border_cell_is_owned_by_attention_pane_uses_attention_style() -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let active_pane = PaneId::new(1)?;
        let attention_pane = PaneId::new(2)?;
        let attention_region = self::pane_region(attention_pane, 0, 0, 3, 1, 1);
        let border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Vertical,
            PanePosition { row: 0, col: 1 },
            3,
            std::slice::from_ref(&attention_region),
            &[],
        )?;

        let border_cells = self::compose_border_cells(
            std::slice::from_ref(&border),
            Some(&active_pane),
            std::slice::from_ref(&attention_pane),
            BorderRenderMode::Focus,
        )?;
        self::paste_borders(
            &mut rows,
            MuxrConfig::default().pane_borders,
            MuxrConfig::default().pane_attention,
            &[border],
            Some(&active_pane),
            std::slice::from_ref(&attention_pane),
            BorderRenderMode::Focus,
        )?;

        let border_cell = rows
            .first()
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected attention border cell"))?;

        assert_that!(border_cell.text(), eq("│"));
        assert_that!(
            self::border_cell_at(&border_cells, 0, 1)?.visual,
            eq(BorderVisual::Attention)
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case::focus(BorderRenderMode::Focus, BorderVisual::Attention)]
    #[case::resize(BorderRenderMode::Resize, BorderVisual::Resize)]
    fn test_paste_borders_when_shared_border_touches_active_and_attention_panes_uses_mode_style(
        #[case] mode: BorderRenderMode,
        #[case] expected_visual: BorderVisual,
    ) -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let active_pane = PaneId::new(1)?;
        let attention_pane = PaneId::new(2)?;
        let active_region = self::pane_region(active_pane, 0, 0, 3, 1, 1);
        let attention_region = self::pane_region(attention_pane, 0, 2, 3, 1, 1);
        let border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Vertical,
            PanePosition { row: 0, col: 1 },
            3,
            std::slice::from_ref(&active_region),
            std::slice::from_ref(&attention_region),
        )?;

        let border_cells = self::compose_border_cells(
            std::slice::from_ref(&border),
            Some(&active_pane),
            std::slice::from_ref(&attention_pane),
            mode,
        )?;
        self::paste_borders(
            &mut rows,
            MuxrConfig::default().pane_borders,
            MuxrConfig::default().pane_attention,
            &[border],
            Some(&active_pane),
            std::slice::from_ref(&attention_pane),
            mode,
        )?;

        let border_cell = rows
            .first()
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected shared active attention border cell"))?;

        assert_that!(border_cell.text(), eq("│"));
        assert_that!(self::border_cell_at(&border_cells, 0, 1)?.visual, eq(expected_visual));
        Ok(())
    }

    #[rstest::rstest]
    #[case::focus(BorderRenderMode::Focus, BorderVisual::Focused)]
    #[case::resize(BorderRenderMode::Resize, BorderVisual::Resize)]
    fn test_paste_borders_when_attention_pane_is_active_uses_active_mode_style(
        #[case] mode: BorderRenderMode,
        #[case] expected_visual: BorderVisual,
    ) -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let active_pane = PaneId::new(1)?;
        let active_region = self::pane_region(active_pane, 0, 0, 3, 1, 1);
        let border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Vertical,
            PanePosition { row: 0, col: 1 },
            3,
            std::slice::from_ref(&active_region),
            &[],
        )?;

        let border_cells = self::compose_border_cells(
            std::slice::from_ref(&border),
            Some(&active_pane),
            std::slice::from_ref(&active_pane),
            mode,
        )?;
        self::paste_borders(
            &mut rows,
            MuxrConfig::default().pane_borders,
            MuxrConfig::default().pane_attention,
            &[border],
            Some(&active_pane),
            std::slice::from_ref(&active_pane),
            mode,
        )?;

        let border_cell = rows
            .first()
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected active attention border cell"))?;

        assert_that!(border_cell.text(), eq("│"));
        assert_that!(self::border_cell_at(&border_cells, 0, 1)?.visual, eq(expected_visual));
        Ok(())
    }

    #[rstest::rstest]
    #[case::focus(BorderRenderMode::Focus, BorderVisual::Focused)]
    #[case::resize(BorderRenderMode::Resize, BorderVisual::Resize)]
    fn test_paste_borders_when_crossing_border_segment_is_owned_by_active_pane_uses_mode_style(
        #[case] mode: BorderRenderMode,
        #[case] expected_visual: BorderVisual,
    ) -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let active_pane = PaneId::new(1)?;
        let active_region = self::pane_region(active_pane, 1, 0, 1, 3, 1);
        let horizontal_border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Horizontal,
            PanePosition { row: 0, col: 0 },
            3,
            &[],
            std::slice::from_ref(&active_region),
        )?;

        let borders = vec![
            PaneBorder::with_adjacent_regions(PaneBorderAxis::Vertical, PanePosition { row: 0, col: 0 }, 3, &[], &[])?,
            horizontal_border,
        ];
        let border_cells = self::compose_border_cells(&borders, Some(&active_pane), &[], mode)?;
        self::paste_borders(
            &mut rows,
            MuxrConfig::default().pane_borders,
            MuxrConfig::default().pane_attention,
            &borders,
            Some(&active_pane),
            &[],
            mode,
        )?;

        let border_cell = rows
            .first()
            .and_then(|row| row.first())
            .ok_or_else(|| report!("expected active junction border cell"))?;

        assert_that!(border_cell.text(), eq("┼"));
        assert_that!(self::border_cell_at(&border_cells, 0, 0)?.visual, eq(expected_visual));
        Ok(())
    }

    #[rstest::rstest]
    #[case::focus(BorderRenderMode::Focus, BorderVisual::Focused)]
    #[case::resize(BorderRenderMode::Resize, BorderVisual::Resize)]
    fn test_paste_borders_when_parent_border_spans_nested_panes_highlights_corner_and_owned_cells(
        #[case] mode: BorderRenderMode,
        #[case] expected_visual: BorderVisual,
    ) -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let top_right_pane = PaneId::new(1)?;
        let active_pane = PaneId::new(2)?;
        let top_right_region = self::pane_region(top_right_pane, 0, 2, 1, 1, 1);
        let active_region = self::pane_region(active_pane, 2, 2, 1, 1, 2);
        let border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Vertical,
            PanePosition { row: 0, col: 1 },
            3,
            &[],
            &[top_right_region, active_region],
        )?;

        let border_cells = self::compose_border_cells(std::slice::from_ref(&border), Some(&active_pane), &[], mode)?;
        self::paste_borders(
            &mut rows,
            MuxrConfig::default().pane_borders,
            MuxrConfig::default().pane_attention,
            &[border],
            Some(&active_pane),
            &[],
            mode,
        )?;

        let top_segment = rows
            .first()
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected top vertical border segment"))?;
        let corner_segment = rows
            .get(1)
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected active corner border segment"))?;
        let active_segment = rows
            .get(2)
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected active vertical border segment"))?;

        assert_that!(top_segment.text(), eq("│"));
        assert_that!(corner_segment.text(), eq("│"));
        assert_that!(active_segment.text(), eq("│"));
        assert_that!(
            self::border_cell_at(&border_cells, 0, 1)?.visual,
            eq(BorderVisual::Default)
        );
        assert_that!(self::border_cell_at(&border_cells, 1, 1)?.visual, eq(expected_visual));
        assert_that!(self::border_cell_at(&border_cells, 2, 1)?.visual, eq(expected_visual));
        Ok(())
    }

    fn empty_render_rows(size: &TerminalSize) -> Vec<Vec<RenderCell>> {
        let blank = RenderCell::narrow(" ", RenderStyle::default());
        (0..size.rows())
            .map(|_| vec![blank.clone(); usize::from(size.cols())])
            .collect()
    }

    fn border_cell_at(
        cells: &std::collections::BTreeMap<(u16, u16), BorderCell>,
        row: u16,
        col: u16,
    ) -> rootcause::Result<&BorderCell> {
        cells
            .get(&(row, col))
            .ok_or_else(|| report!("expected border cell").attach(format!("row={row} col={col}")))
    }
}
