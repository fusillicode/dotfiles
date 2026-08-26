use std::collections::HashMap;

use muxr_config::ScrollbackDumpStyle;
use muxr_core::RenderCell;
use muxr_core::RenderCellWidth;
use muxr_core::RenderColor;
use muxr_core::RenderCursor;
use muxr_core::RenderCursorShape;
use muxr_core::RenderHyperlink;
use muxr_core::RenderRowSpan;
use muxr_core::RenderStyle;
use muxr_core::RenderTextStyle;
use muxr_core::RowWrap;
use muxr_core::TerminalSize;
use rio_vt::ansi::CursorShape as RioCursorShape;
use rio_vt::config::colors::AnsiColor;
use rio_vt::config::colors::NamedColor;
use rio_vt::crosswords::Crosswords;
use rio_vt::crosswords::grid::Grid;
use rio_vt::crosswords::grid::row::Row;
use rio_vt::crosswords::pos::Column;
use rio_vt::crosswords::pos::Line;
use rio_vt::crosswords::pos::Pos;
use rio_vt::crosswords::square::CellFlags;
use rio_vt::crosswords::square::ContentTag;
use rio_vt::crosswords::square::ExtrasId;
use rio_vt::crosswords::square::Square;
use rio_vt::crosswords::square::Wide;
use rio_vt::crosswords::style::StyleFlags;
use rio_vt::event::EventListener;
use rootcause::prelude::ResultExt;
use smallvec::SmallVec;

use super::CursorShapeSource;
use super::TerminalSnapshot;
use super::TerminalSnapshotScope;

const RENDER_CELL_TEXT_INLINE_BYTES: usize = 24;

pub(super) fn snapshot_rows<U: EventListener>(
    terminal: &Crosswords<U>,
    scope: TerminalSnapshotScope,
    cursor_shape_source: CursorShapeSource,
) -> rootcause::Result<TerminalSnapshot> {
    let screen_rows = u16::try_from(terminal.screen_lines()).context("muxr terminal row count overflowed")?;
    let screen_cols = u16::try_from(terminal.columns()).context("muxr terminal column count overflowed")?;
    let size = TerminalSize::new(screen_cols, screen_rows)?;
    let rio_cursor = terminal.cursor();
    let cursor_row = u16::try_from(rio_cursor.pos.row.0).context("muxr terminal cursor row overflowed")?;
    let cursor_col = u16::try_from(rio_cursor.pos.col.0).context("muxr terminal cursor column overflowed")?;

    let cursor_visible = terminal.display_offset() == 0
        && rio_cursor.is_visible()
        && cursor_row < screen_rows
        && cursor_col < screen_cols;

    let cursor = RenderCursor {
        row: cursor_row,
        col: cursor_col,
        shape: render_cursor_shape(rio_cursor.content, terminal.blinking_cursor, cursor_shape_source),
        visibility: if cursor_visible {
            muxr_core::RenderCursorVisibility::Visible
        } else {
            muxr_core::RenderCursorVisibility::Hidden
        },
    };

    let row_wraps = (0..terminal.screen_lines())
        .map(|row| row_wrap(&terminal.grid[visible_line(terminal, row)]))
        .collect();

    let mut hyperlink_cache = HashMap::new();
    let rows = (0..terminal.screen_lines())
        .filter(|row| matches!(scope, TerminalSnapshotScope::Full) || terminal.grid[visible_line(terminal, *row)].dirty)
        .map(|row| {
            let line = visible_line(terminal, row);
            RenderRowSpan::new(
                u16::try_from(row).context("muxr terminal snapshot row index overflowed")?,
                0,
                render_row(&terminal.grid, line, usize::from(screen_cols), &mut hyperlink_cache),
            )
        })
        .collect::<rootcause::Result<Vec<_>>>()?;

    Ok(TerminalSnapshot {
        cursor,
        row_wraps,
        rows,
        scope,
        size,
    })
}

pub(super) fn visible_line<U: EventListener>(terminal: &Crosswords<U>, row: usize) -> Line {
    let row = i32::try_from(row).unwrap_or(i32::MAX);
    let offset = i32::try_from(terminal.display_offset()).unwrap_or(i32::MAX);
    Line(row.saturating_sub(offset))
}

pub(super) fn scrollback_grid_dump<U: EventListener>(
    terminal: &Crosswords<U>,
    lines: std::ops::Range<i32>,
    style: ScrollbackDumpStyle,
) -> Vec<u8> {
    let mut dump = Vec::new();
    let mut hyperlink_cache = HashMap::new();
    for line in lines {
        let row = render_row(&terminal.grid, Line(line), terminal.columns(), &mut hyperlink_cache);
        append_scrollback_dump_row(&row, style, &mut dump);
    }
    dump
}

pub(super) fn row_wrap(row: &Row<Square>) -> RowWrap {
    if row
        .inner
        .last()
        .is_some_and(|square| square.contains_cell_flag(CellFlags::WRAPLINE))
    {
        RowWrap::EndsWithSoftWrap
    } else {
        RowWrap::EndsBeforeSoftWrap
    }
}

fn render_row(
    grid: &Grid<Square>,
    line: Line,
    columns: usize,
    hyperlink_cache: &mut HashMap<ExtrasId, Option<RenderHyperlink>>,
) -> Vec<RenderCell> {
    let row = &grid[line];
    (0..columns)
        .map(|col| {
            row.inner.get(col).map_or_else(
                || RenderCell::narrow(" ", RenderStyle::default()),
                |square| render_cell(grid, line, Column(col), *square, hyperlink_cache),
            )
        })
        .collect()
}

fn render_cell(
    grid: &Grid<Square>,
    line: Line,
    col: Column,
    square: Square,
    hyperlink_cache: &mut HashMap<ExtrasId, Option<RenderHyperlink>>,
) -> RenderCell {
    let style = render_style(grid, square);
    let width = square.wide();

    let mut cell = match width {
        Wide::Spacer => RenderCell::wide_continuation(style),
        Wide::Wide | Wide::LeadingSpacer | Wide::Narrow if square.is_bg_only() => render_text_cell(width, " ", style),
        Wide::Wide | Wide::LeadingSpacer | Wide::Narrow if square.has_grapheme() => {
            let text = square_text(grid, line, col);
            render_text_cell(
                width,
                std::str::from_utf8(text.as_slice()).map_or(" ", |text| text),
                style,
            )
        }
        Wide::Wide | Wide::LeadingSpacer | Wide::Narrow => {
            let character = normalized_render_character(square.c());
            let mut encoded = [0_u8; char::MAX_LEN_UTF8];
            render_text_cell(width, character.encode_utf8(&mut encoded), style)
        }
    };

    if square.content_tag() == ContentTag::Codepoint {
        let hyperlink = square.extras_id().and_then(|id| {
            grid.extras_table
                .get(id)
                .and_then(|extras| extras.hyperlink.as_ref())
                .map(|hyperlink| (id, hyperlink))
        });

        if let Some((id, hyperlink)) = hyperlink {
            let render_hyperlink = hyperlink_cache
                .entry(id)
                .or_insert_with(|| RenderHyperlink::new(hyperlink.uri().to_owned()).ok());
            if let Some(render_hyperlink) = render_hyperlink {
                cell = cell.with_hyperlink(render_hyperlink.clone());
            }
        }
    }
    cell
}

fn render_text_cell(width: Wide, text: &str, style: RenderStyle) -> RenderCell {
    match width {
        Wide::Wide => RenderCell::wide(text, style),
        Wide::Spacer => RenderCell::wide_continuation(style),
        Wide::LeadingSpacer | Wide::Narrow => RenderCell::narrow(text, style),
    }
}

fn square_text(grid: &Grid<Square>, line: Line, col: Column) -> SmallVec<[u8; RENDER_CELL_TEXT_INLINE_BYTES]> {
    let mut text = SmallVec::new();
    for character in grid.cell_text(Pos::new(line, col)) {
        let character = normalized_render_character(character);
        let mut encoded = [0_u8; char::MAX_LEN_UTF8];
        text.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
    }
    if text.is_empty() {
        text.push(b' ');
    }
    text
}

const fn normalized_render_character(character: char) -> char {
    if matches!(character, '\0' | '\t') {
        ' '
    } else {
        character
    }
}

fn render_style(grid: &Grid<Square>, square: Square) -> RenderStyle {
    if square.is_bg_only() {
        return RenderStyle {
            attrs: RenderTextStyle::empty(),
            bg: match square.content_tag() {
                ContentTag::BgPalette => RenderColor::Indexed(square.bg_palette_index()),
                ContentTag::BgRgb => {
                    let (r, g, b) = square.bg_rgb();
                    RenderColor::Rgb { r, g, b }
                }
                ContentTag::Codepoint => RenderColor::Default,
            },
            fg: RenderColor::Default,
        };
    }

    let style = grid.style_of(&square);
    RenderStyle {
        attrs: RenderTextStyle::empty()
            .set_bold(style.flags.contains(StyleFlags::BOLD))
            .set_dim(style.flags.contains(StyleFlags::DIM))
            .set_italic(style.flags.contains(StyleFlags::ITALIC))
            .set_underline(style.flags.intersects(StyleFlags::ALL_UNDERLINES))
            .set_inverse(style.flags.contains(StyleFlags::INVERSE)),
        bg: render_color(style.bg),
        fg: render_color(style.fg),
    }
}

fn render_color(color: AnsiColor) -> RenderColor {
    match color {
        AnsiColor::Indexed(index) => RenderColor::Indexed(index),
        AnsiColor::Spec(rgb) => RenderColor::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        },
        AnsiColor::Named(named) => render_named_color(named),
    }
}

fn render_named_color(color: NamedColor) -> RenderColor {
    let index = color as u16;
    u8::try_from(index)
        .ok()
        .filter(|index| *index <= 15)
        .map_or(RenderColor::Default, RenderColor::Indexed)
}

const fn render_cursor_shape(shape: RioCursorShape, blinking: bool, source: CursorShapeSource) -> RenderCursorShape {
    match source {
        CursorShapeSource::Default => RenderCursorShape::Default,
        CursorShapeSource::Explicit => match (shape, blinking) {
            (RioCursorShape::Beam, false) => RenderCursorShape::SteadyBar,
            (RioCursorShape::Beam, true) => RenderCursorShape::BlinkingBar,
            (RioCursorShape::Block, false) => RenderCursorShape::SteadyBlock,
            (RioCursorShape::Block, true) => RenderCursorShape::BlinkingBlock,
            (RioCursorShape::Underline, false) => RenderCursorShape::SteadyUnderline,
            (RioCursorShape::Underline, true) => RenderCursorShape::BlinkingUnderline,
            (RioCursorShape::Hidden, _) => RenderCursorShape::Default,
        },
    }
}

fn append_scrollback_dump_row(row: &[RenderCell], style: ScrollbackDumpStyle, bytes: &mut Vec<u8>) {
    match style {
        ScrollbackDumpStyle::PlainText => encode_plain_scrollback_dump_row(row, bytes),
        ScrollbackDumpStyle::Ansi => encode_ansi_scrollback_dump_row(row, bytes),
    }
    bytes.push(b'\n');
}

fn encode_plain_scrollback_dump_row(row: &[RenderCell], bytes: &mut Vec<u8>) {
    for cell in trimmed_dump_cells(row) {
        if cell.width() == RenderCellWidth::WideContinuation {
            continue;
        }
        bytes.extend_from_slice(cell.text().as_bytes());
    }
}

fn encode_ansi_scrollback_dump_row(row: &[RenderCell], bytes: &mut Vec<u8>) {
    let mut active_style = RenderStyle::default();
    for cell in trimmed_dump_cells(row) {
        if cell.width() == RenderCellWidth::WideContinuation {
            continue;
        }
        if cell.style() != active_style {
            push_sgr(cell.style(), bytes);
            active_style = cell.style();
        }
        bytes.extend_from_slice(cell.text().as_bytes());
    }
    if active_style != RenderStyle::default() {
        bytes.extend_from_slice(b"\x1b[0m");
    }
}

fn trimmed_dump_cells(row: &[RenderCell]) -> &[RenderCell] {
    let mut cells = row;
    while let Some((last, rest)) = cells.split_last() {
        if last.width() == RenderCellWidth::WideContinuation || last.text() != " " {
            break;
        }
        cells = rest;
    }
    cells
}

fn push_sgr(style: RenderStyle, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(b"\x1b[0");
    push_text_style_sgr(style.attrs, bytes);
    push_color_sgr(38, style.fg, bytes);
    push_color_sgr(48, style.bg, bytes);
    bytes.push(b'm');
}

fn push_text_style_sgr(attrs: RenderTextStyle, bytes: &mut Vec<u8>) {
    for (enabled, code) in [
        (attrs.bold(), "1"),
        (attrs.dim(), "2"),
        (attrs.italic(), "3"),
        (attrs.underline(), "4"),
        (attrs.inverse(), "7"),
    ] {
        if enabled {
            bytes.push(b';');
            bytes.extend_from_slice(code.as_bytes());
        }
    }
}

fn push_color_sgr(prefix: u8, color: RenderColor, bytes: &mut Vec<u8>) {
    match color {
        RenderColor::Default => {}
        RenderColor::Indexed(index) => {
            bytes.push(b';');
            bytes.extend_from_slice(prefix.to_string().as_bytes());
            bytes.extend_from_slice(b";5;");
            bytes.extend_from_slice(index.to_string().as_bytes());
        }
        RenderColor::Rgb { r, g, b } => {
            bytes.push(b';');
            bytes.extend_from_slice(prefix.to_string().as_bytes());
            bytes.extend_from_slice(b";2;");
            bytes.extend_from_slice(r.to_string().as_bytes());
            bytes.push(b';');
            bytes.extend_from_slice(g.to_string().as_bytes());
            bytes.push(b';');
            bytes.extend_from_slice(b.to_string().as_bytes());
        }
    }
}
