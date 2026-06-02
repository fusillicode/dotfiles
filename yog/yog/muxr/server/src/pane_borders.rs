use std::collections::BTreeMap;

use muxr_core::PaneId;
use muxr_core::RenderCell;
use muxr_core::RenderColor;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use rootcause::report;

use crate::pane_layout::PanePosition;
use crate::pane_layout::PaneRegion;

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
                .filter(|region| self::pane_region_owns_border_cell(region, axis, cell_position))
                .map(|region| region.id.clone())
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

    pub fn is_owned_by(&self, position: PanePosition, pane_id: &PaneId) -> bool {
        let Some(offset) = self.cell_offset(position) else {
            return false;
        };

        self.owner_cells.iter().any(|cell| {
            cell.offset == offset
                && cell
                    .pane_ids
                    .iter()
                    .any(|candidate_pane_id| candidate_pane_id == pane_id)
        })
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

fn pane_region_owns_border_cell(region: &PaneRegion, axis: PaneBorderAxis, position: PanePosition) -> bool {
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
            touches_top || touches_bottom
        }
        PaneBorderAxis::Vertical => {
            let row = u32::from(position.row);
            let start_row = u32::from(region.area.origin.row).saturating_sub(1);
            let end_row = region.area.end_row_exclusive();
            let contains_row = row >= start_row && row <= end_row;
            let touches_left = position.col.checked_add(1) == Some(region.area.origin.col) && contains_row;
            let touches_right = region.area.end_col() == Some(position.col) && contains_row;
            touches_left || touches_right
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BorderRenderMode {
    Focus,
    Resize,
}

pub fn paste_borders(
    rows: &mut [Vec<RenderCell>],
    borders: &[PaneBorder],
    active_pane: Option<&PaneId>,
    border_mode: BorderRenderMode,
) -> rootcause::Result<()> {
    let border_cells = self::compose_border_cells(borders, active_pane, border_mode)?;
    for ((row, col), cell) in border_cells {
        let target_row = rows
            .get_mut(usize::from(row))
            .ok_or_else(|| report!("muxr pane border row outside composite frame"))?;
        let target = target_row
            .get_mut(usize::from(col))
            .ok_or_else(|| report!("muxr pane border col outside composite frame"))?;
        *target = RenderCell::narrow(cell.shape.glyph(), cell.style);
    }
    Ok(())
}

fn compose_border_cells(
    borders: &[PaneBorder],
    active_pane: Option<&PaneId>,
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
                    let style = self::border_style_for_cell(border, border.row(), col, active_pane, border_mode);
                    self::add_border_cell(&mut cells, border.row(), col, border.axis(), style);
                }
            }
            PaneBorderAxis::Vertical => {
                let end_row = border
                    .row()
                    .checked_add(border.len())
                    .ok_or_else(|| report!("muxr vertical pane border end overflowed"))?;
                for row in border.row()..end_row {
                    let style = self::border_style_for_cell(border, row, border.col(), active_pane, border_mode);
                    self::add_border_cell(&mut cells, row, border.col(), border.axis(), style);
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
    style: RenderStyle,
) {
    cells
        .entry((row, col))
        .and_modify(|cell| {
            cell.shape = cell.shape.with_axis(axis);
            cell.style = self::stronger_border_style(cell.style, style);
        })
        .or_insert_with(|| BorderCell {
            shape: BorderCellShape::from_axis(axis),
            style,
        });
}

fn add_adjacent_border_junctions(cells: &mut BTreeMap<(u16, u16), BorderCell>) {
    // Nested splits place child borders next to parent borders, not always on the same cell; compose those
    // neighbor edges so the rendered frame uses connected junction glyphs instead of detached line segments.
    let base_cells = cells.clone();
    for ((row, col), cell) in &base_cells {
        if cell.shape.has_horizontal() {
            if let Some(left_col) = col.checked_sub(1)
                && base_cells
                    .get(&(*row, left_col))
                    .is_some_and(|neighbor| neighbor.shape.has_vertical())
            {
                self::merge_border_edge(cells, (*row, left_col), BorderCellEdge::Right, cell.style);
            }
            if let Some(right_col) = col.checked_add(1)
                && base_cells
                    .get(&(*row, right_col))
                    .is_some_and(|neighbor| neighbor.shape.has_vertical())
            {
                self::merge_border_edge(cells, (*row, right_col), BorderCellEdge::Left, cell.style);
            }
        }
        if cell.shape.has_vertical() {
            if let Some(up_row) = row.checked_sub(1)
                && base_cells
                    .get(&(up_row, *col))
                    .is_some_and(|neighbor| neighbor.shape.has_horizontal())
            {
                self::merge_border_edge(cells, (up_row, *col), BorderCellEdge::Down, cell.style);
            }
            if let Some(down_row) = row.checked_add(1)
                && base_cells
                    .get(&(down_row, *col))
                    .is_some_and(|neighbor| neighbor.shape.has_horizontal())
            {
                self::merge_border_edge(cells, (down_row, *col), BorderCellEdge::Up, cell.style);
            }
        }
    }
}

fn merge_border_edge(
    cells: &mut BTreeMap<(u16, u16), BorderCell>,
    position: (u16, u16),
    edge: BorderCellEdge,
    style: RenderStyle,
) {
    if let Some(cell) = cells.get_mut(&position) {
        cell.shape = cell.shape.with_edge(edge);
        cell.style = self::stronger_border_style(cell.style, style);
    }
}

fn border_style_for_cell(
    border: &PaneBorder,
    row: u16,
    col: u16,
    active_pane: Option<&PaneId>,
    border_mode: BorderRenderMode,
) -> RenderStyle {
    if active_pane.is_some_and(|pane_id| border.is_owned_by(PanePosition { row, col }, pane_id)) {
        return match border_mode {
            BorderRenderMode::Focus => self::focused_border_style(),
            BorderRenderMode::Resize => self::resize_border_style(),
        };
    }

    self::border_style()
}

fn stronger_border_style(current: RenderStyle, incoming: RenderStyle) -> RenderStyle {
    if current == self::resize_border_style() || incoming == self::resize_border_style() {
        self::resize_border_style()
    } else if current == self::focused_border_style() || incoming == self::focused_border_style() {
        self::focused_border_style()
    } else {
        incoming
    }
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
            PaneBorderAxis::Horizontal => Self::from_edges((self.up(), self.down(), true, true)),
            PaneBorderAxis::Vertical => Self::from_edges((true, true, self.left(), self.right())),
        }
    }

    const fn with_edge(self, edge: BorderCellEdge) -> Self {
        match edge {
            BorderCellEdge::Down => Self::from_edges((self.up(), true, self.left(), self.right())),
            BorderCellEdge::Left => Self::from_edges((self.up(), self.down(), true, self.right())),
            BorderCellEdge::Right => Self::from_edges((self.up(), self.down(), self.left(), true)),
            BorderCellEdge::Up => Self::from_edges((true, self.down(), self.left(), self.right())),
        }
    }

    const fn has_horizontal(self) -> bool {
        self.left() || self.right()
    }

    const fn has_vertical(self) -> bool {
        self.up() || self.down()
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

    const fn from_edges(edges: (bool, bool, bool, bool)) -> Self {
        match edges {
            (false, false, false, false) => Self::Empty,
            (true, false, false, false) => Self::Up,
            (false, true, false, false) => Self::Down,
            (false, false, true, false) => Self::Left,
            (false, false, false, true) => Self::Right,
            (true, true, false, false) => Self::Vertical,
            (false, false, true, true) => Self::Horizontal,
            (true, false, true, false) => Self::UpLeft,
            (true, false, false, true) => Self::UpRight,
            (false, true, true, false) => Self::DownLeft,
            (false, true, false, true) => Self::DownRight,
            (true, true, true, false) => Self::UpDownLeft,
            (true, true, false, true) => Self::UpDownRight,
            (true, false, true, true) => Self::UpLeftRight,
            (false, true, true, true) => Self::DownLeftRight,
            (true, true, true, true) => Self::All,
        }
    }

    const fn up(self) -> bool {
        match self {
            Self::All
            | Self::Up
            | Self::UpDownLeft
            | Self::UpDownRight
            | Self::UpLeft
            | Self::UpLeftRight
            | Self::UpRight
            | Self::Vertical => true,
            Self::Down
            | Self::DownLeft
            | Self::DownLeftRight
            | Self::DownRight
            | Self::Empty
            | Self::Horizontal
            | Self::Left
            | Self::Right => false,
        }
    }

    const fn down(self) -> bool {
        match self {
            Self::All
            | Self::Down
            | Self::DownLeft
            | Self::DownLeftRight
            | Self::DownRight
            | Self::UpDownLeft
            | Self::UpDownRight
            | Self::Vertical => true,
            Self::Empty
            | Self::Horizontal
            | Self::Left
            | Self::Right
            | Self::Up
            | Self::UpLeft
            | Self::UpLeftRight
            | Self::UpRight => false,
        }
    }

    const fn left(self) -> bool {
        match self {
            Self::All
            | Self::DownLeft
            | Self::DownLeftRight
            | Self::Horizontal
            | Self::Left
            | Self::UpDownLeft
            | Self::UpLeft
            | Self::UpLeftRight => true,
            Self::Down
            | Self::DownRight
            | Self::Empty
            | Self::Right
            | Self::Up
            | Self::UpDownRight
            | Self::UpRight
            | Self::Vertical => false,
        }
    }

    const fn right(self) -> bool {
        match self {
            Self::All
            | Self::DownLeftRight
            | Self::DownRight
            | Self::Horizontal
            | Self::Right
            | Self::UpDownRight
            | Self::UpLeftRight
            | Self::UpRight => true,
            Self::Down
            | Self::DownLeft
            | Self::Empty
            | Self::Left
            | Self::Up
            | Self::UpDownLeft
            | Self::UpLeft
            | Self::Vertical => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BorderCell {
    shape: BorderCellShape,
    style: RenderStyle,
}

// Pane borders can use exact RGB for pane state contrast; indexed colors remain useful for palette-sized accents.
const fn border_style() -> RenderStyle {
    RenderStyle {
        attrs: RenderTextStyle::empty().set_dim(true),
        bg: RenderColor::Default,
        fg: RenderColor::Rgb { r: 50, g: 50, b: 50 },
    }
}

const fn focused_border_style() -> RenderStyle {
    RenderStyle {
        attrs: RenderTextStyle::empty().set_bold(true),
        bg: RenderColor::Default,
        fg: RenderColor::Rgb { r: 132, g: 132, b: 132 },
    }
}

const fn resize_border_style() -> RenderStyle {
    RenderStyle {
        attrs: RenderTextStyle::empty().set_bold(true),
        bg: RenderColor::Default,
        fg: RenderColor::Indexed(166),
    }
}

#[cfg(test)]
mod tests {
    use muxr_core::PaneId;
    use muxr_core::SessionName;
    use muxr_core::TerminalSize;
    use rootcause::report;

    use super::*;
    use crate::pane_focus::PaneFocusDirection;
    use crate::pane_layout::PaneArea;
    use crate::pane_layout::PanePosition;
    use crate::pane_layout::PaneRegion;
    use crate::pane_layout::PaneSize;
    use crate::pane_split::PaneSplitAxis;
    use crate::state::SessionLayout;
    use crate::state::SessionMetadata;

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
        let active_pane = PaneId::new("pane-active")?;
        let active_region = self::pane_region(active_pane.clone(), 2, 2, 1, 1, 1);
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

        assert2::assert!(border.is_owned_by(
            PanePosition {
                row: cell_row,
                col: cell_col
            },
            &active_pane
        ));
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
        let active_pane = PaneId::new("pane-active")?;
        let active_region = self::pane_region(active_pane.clone(), 2, 2, 1, 1, 1);
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

        assert2::assert!(!border.is_owned_by(
            PanePosition {
                row: cell_row,
                col: cell_col
            },
            &active_pane
        ));
        Ok(())
    }

    #[rstest::rstest]
    #[case::vertical_then_horizontal_focus_lower_right(
        PaneSplitAxis::Vertical,
        PaneSplitAxis::Horizontal,
        None,
        vec![(12, 40, "├"), (13, 40, "│"), (12, 41, "─")],
    )]
    #[case::vertical_then_horizontal_focus_upper_right(
        PaneSplitAxis::Vertical,
        PaneSplitAxis::Horizontal,
        Some(PaneFocusDirection::Up),
        vec![(0, 40, "│"), (11, 40, "│"), (12, 40, "├"), (12, 41, "─")],
    )]
    #[case::horizontal_then_vertical_focus_lower_right(
        PaneSplitAxis::Horizontal,
        PaneSplitAxis::Vertical,
        None,
        vec![(12, 40, "┬"), (12, 41, "─"), (13, 40, "│")],
    )]
    #[case::horizontal_then_vertical_focus_lower_left(
        PaneSplitAxis::Horizontal,
        PaneSplitAxis::Vertical,
        Some(PaneFocusDirection::Left),
        vec![(12, 39, "─"), (12, 40, "┬"), (13, 40, "│")],
    )]
    fn test_layout_nested_split_border_rendering_when_active_pane_touches_parent_and_child_borders_highlights_outline(
        #[case] first_axis: PaneSplitAxis,
        #[case] second_axis: PaneSplitAxis,
        #[case] focus_direction: Option<PaneFocusDirection>,
        #[case] expected_focused_cells: Vec<(u16, u16, &'static str)>,
    ) -> rootcause::Result<()> {
        let session: SessionName = "work".parse()?;
        let size = TerminalSize::new(80, 24)?;
        let mut layout = SessionLayout::initial(&session, self::metadata("sh", 1))?;

        layout.split_active_pane(self::metadata("sh", 2), first_axis)?;
        layout.split_active_pane(self::metadata("sh", 3), second_axis)?;
        if let Some(direction) = focus_direction {
            assert2::assert!(layout.focus_pane_direction(&size, direction)?);
        }

        let pane_layout = layout.pane_layout(&size)?;
        let active_pane = layout.active_pane_id()?;
        let mut rows = self::empty_render_rows(&size);
        self::paste_borders(
            &mut rows,
            pane_layout.borders(),
            Some(&active_pane),
            BorderRenderMode::Focus,
        )?;

        for (row, col, glyph) in expected_focused_cells {
            let cell = rows
                .get(usize::from(row))
                .and_then(|row| row.get(usize::from(col)))
                .ok_or_else(|| report!("expected focused border cell").attach(format!("row={row} col={col}")))?;
            pretty_assertions::assert_eq!(cell.text(), glyph);
            pretty_assertions::assert_eq!(cell.style(), self::focused_border_style());
        }
        Ok(())
    }

    #[test]
    fn test_paste_borders_when_borders_are_rendered_uses_box_drawing_style() -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);

        self::paste_borders(
            &mut rows,
            &[
                PaneBorder::with_adjacent_regions(
                    PaneBorderAxis::Vertical,
                    PanePosition { row: 0, col: 1 },
                    3,
                    &[],
                    &[],
                )?,
                PaneBorder::with_adjacent_regions(
                    PaneBorderAxis::Horizontal,
                    PanePosition { row: 1, col: 0 },
                    3,
                    &[],
                    &[],
                )?,
            ],
            None,
            BorderRenderMode::Focus,
        )?;

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

        pretty_assertions::assert_eq!(vertical_cell.text(), "│");
        pretty_assertions::assert_eq!(horizontal_cell.text(), "─");
        pretty_assertions::assert_eq!(junction_cell.text(), "┼");
        pretty_assertions::assert_eq!(vertical_cell.style(), self::border_style());
        Ok(())
    }

    #[rstest::rstest]
    #[case::focus(BorderRenderMode::Focus, self::focused_border_style())]
    #[case::resize(BorderRenderMode::Resize, self::resize_border_style())]
    fn test_paste_borders_when_border_cell_is_owned_by_active_pane_uses_mode_style(
        #[case] mode: BorderRenderMode,
        #[case] expected_style: RenderStyle,
    ) -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let active_pane = PaneId::new("pane-1")?;
        let active_region = self::pane_region(active_pane.clone(), 0, 0, 3, 1, 1);
        let border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Vertical,
            PanePosition { row: 0, col: 1 },
            3,
            std::slice::from_ref(&active_region),
            &[],
        )?;

        self::paste_borders(&mut rows, &[border], Some(&active_pane), mode)?;

        let border_cell = rows
            .first()
            .and_then(|row| row.get(1))
            .ok_or_else(|| report!("expected active border cell"))?;

        pretty_assertions::assert_eq!(border_cell.style(), expected_style);
        Ok(())
    }

    #[rstest::rstest]
    #[case::focus(BorderRenderMode::Focus, self::focused_border_style())]
    #[case::resize(BorderRenderMode::Resize, self::resize_border_style())]
    fn test_paste_borders_when_crossing_border_segment_is_owned_by_active_pane_uses_mode_style(
        #[case] mode: BorderRenderMode,
        #[case] expected_style: RenderStyle,
    ) -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let active_pane = PaneId::new("pane-1")?;
        let active_region = self::pane_region(active_pane.clone(), 1, 0, 1, 3, 1);
        let horizontal_border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Horizontal,
            PanePosition { row: 0, col: 0 },
            3,
            &[],
            std::slice::from_ref(&active_region),
        )?;

        self::paste_borders(
            &mut rows,
            &[
                PaneBorder::with_adjacent_regions(
                    PaneBorderAxis::Vertical,
                    PanePosition { row: 0, col: 0 },
                    3,
                    &[],
                    &[],
                )?,
                horizontal_border,
            ],
            Some(&active_pane),
            mode,
        )?;

        let border_cell = rows
            .first()
            .and_then(|row| row.first())
            .ok_or_else(|| report!("expected active junction border cell"))?;

        pretty_assertions::assert_eq!(border_cell.text(), "┼");
        pretty_assertions::assert_eq!(border_cell.style(), expected_style);
        Ok(())
    }

    #[rstest::rstest]
    #[case::focus(BorderRenderMode::Focus, self::focused_border_style())]
    #[case::resize(BorderRenderMode::Resize, self::resize_border_style())]
    fn test_paste_borders_when_parent_border_spans_nested_panes_highlights_corner_and_owned_cells(
        #[case] mode: BorderRenderMode,
        #[case] expected_style: RenderStyle,
    ) -> rootcause::Result<()> {
        let mut rows = self::empty_render_rows(&TerminalSize::new(3, 3)?);
        let top_right_pane = PaneId::new("pane-top-right")?;
        let active_pane = PaneId::new("pane-bottom-right")?;
        let top_right_region = self::pane_region(top_right_pane, 0, 2, 1, 1, 1);
        let active_region = self::pane_region(active_pane.clone(), 2, 2, 1, 1, 2);
        let border = PaneBorder::with_adjacent_regions(
            PaneBorderAxis::Vertical,
            PanePosition { row: 0, col: 1 },
            3,
            &[],
            &[top_right_region, active_region],
        )?;

        self::paste_borders(&mut rows, &[border], Some(&active_pane), mode)?;

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

        pretty_assertions::assert_eq!(top_segment.style(), self::border_style());
        pretty_assertions::assert_eq!(corner_segment.style(), expected_style);
        pretty_assertions::assert_eq!(active_segment.style(), expected_style);
        Ok(())
    }

    fn empty_render_rows(size: &TerminalSize) -> Vec<Vec<RenderCell>> {
        let blank = RenderCell::narrow(" ", RenderStyle::default());
        (0..size.rows())
            .map(|_| vec![blank.clone(); usize::from(size.cols())])
            .collect()
    }

    fn metadata(command_label: &str, started_at: u64) -> SessionMetadata {
        SessionMetadata {
            command_label: command_label.to_owned(),
            cwd: "/tmp".to_owned(),
            started_at,
        }
    }
}
