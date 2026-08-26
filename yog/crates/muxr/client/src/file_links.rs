use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use muxr_core::ClientMousePosition;
use muxr_core::PaneRegionSnapshot;
use muxr_core::RenderCell;
use muxr_core::RenderCellWidth;
use muxr_core::RowWrap;

use crate::frame_buffer::FrameBuffer;

const PATH_START_PREFIXES: [&str; 4] = ["/", "~/", "./", "../"];

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FileLink {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Clone, Copy)]
enum FileLinkReferenceMode {
    Delimited,
    UnquotedSpaces,
}

#[derive(Clone, Copy)]
enum FileLinkExplicitness {
    Implicit,
    Explicit,
}

#[derive(Clone, Copy)]
enum FileLinkFallbackOrder {
    PreservePath,
    PreserveLocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileLinkExistence {
    Existing,
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileLinkWrap {
    SingleRow,
    SoftWrapped,
}

impl FileLinkWrap {
    const fn join(self, other: Self) -> Self {
        if matches!(self, Self::SoftWrapped) || matches!(other, Self::SoftWrapped) {
            Self::SoftWrapped
        } else {
            Self::SingleRow
        }
    }
}

struct ReferenceParts {
    chars: Vec<String>,
    wrap: FileLinkWrap,
}

struct FileLinkExtraction {
    raw: String,
    raw_reference: String,
    clicked_index: usize,
    explicitness: FileLinkExplicitness,
    wrap: FileLinkWrap,
}

impl FileLink {
    #[cfg(test)]
    fn resolve_from_cwd(&self, cwd: &str) -> Option<Self> {
        self.resolve_with_existence(cwd).map(|resolved| resolved.link)
    }

    fn resolve_with_existence(&self, cwd: &str) -> Option<ResolvedFileLink> {
        let raw_path = self.path.to_str()?;
        let path = self::resolve_path(raw_path, cwd)?;
        let existence = self::file_link_existence(&path);
        // A bare word is ambiguous in TUI prose. Require it to name an existing file or directory when it has no
        // extension, while allowing extension-bearing names so links to files that are about to be created still work.
        if self::is_bare_name(raw_path)
            && matches!(existence, FileLinkExistence::Missing)
            && Path::new(raw_path).extension().is_none()
        {
            return None;
        }
        Some(ResolvedFileLink {
            link: Self {
                path,
                line: self.line,
                column: self.column,
            },
            existence,
        })
    }
}

struct ResolvedFileLink {
    link: FileLink,
    existence: FileLinkExistence,
}

impl FrameBuffer {
    #[cfg(test)]
    fn file_link_at(&self, region: &PaneRegionSnapshot, position: ClientMousePosition) -> Option<FileLink> {
        self.at_with_reference_mode(region, position, FileLinkReferenceMode::Delimited)
            .and_then(|extraction| self::file_link_from_reference(&extraction.raw))
    }

    pub fn file_link_at_resolved(
        &self,
        region: &PaneRegionSnapshot,
        position: ClientMousePosition,
        cwd: &str,
    ) -> Option<FileLink> {
        let unquoted = self.at_with_reference_mode(region, position, FileLinkReferenceMode::UnquotedSpaces);
        let resolved_unquoted = unquoted
            .as_ref()
            .and_then(|extraction| extraction.resolve_unquoted(cwd));
        if resolved_unquoted.is_none()
            && unquoted.as_ref().is_some_and(|extraction| {
                extraction.raw.chars().any(char::is_whitespace)
                    && matches!(extraction.explicitness, FileLinkExplicitness::Implicit)
            })
        {
            return None;
        }
        let delimited = self.at_with_reference_mode(region, position, FileLinkReferenceMode::Delimited);
        let resolved_delimited = delimited.as_ref().and_then(|extraction| extraction.resolve_single(cwd));
        resolved_unquoted.or(resolved_delimited)
    }

    fn at_with_reference_mode(
        &self,
        region: &PaneRegionSnapshot,
        position: ClientMousePosition,
        mode: FileLinkReferenceMode,
    ) -> Option<FileLinkExtraction> {
        if region.containment(position.row, position.col) != muxr_core::PaneRegionContainment::Inside {
            return None;
        }

        let position = self::reference_position_for_click(self, region, position)?;
        let clicked = self::cell_text(self.cell(position.row, position.col))?;
        if !self::is_reference_text(clicked) {
            return None;
        }

        let left = self::collect_left(self, region, position, mode);
        let right = self::collect_right(self, region, position, mode);
        let wrap = left.wrap.join(right.wrap);
        let mut raw = left.chars.iter().rev().fold(String::new(), |mut raw, text| {
            raw.push_str(text);
            raw
        });
        let clicked_index = raw.len();
        raw.push_str(clicked);
        for text in &right.chars {
            raw.push_str(text);
        }

        let raw_start = raw.trim_start();
        let explicitness = if raw_start.starts_with('"')
            || raw_start.starts_with('\'')
            || raw_start.starts_with('`')
            || raw.contains("\\ ")
            || (raw_start.starts_with('[') && self::markdown_target(raw_start).is_some())
        {
            FileLinkExplicitness::Explicit
        } else {
            FileLinkExplicitness::Implicit
        };
        let (trimmed, trim_start) = self::trim_reference(&raw);
        let trimmed_end = trim_start.checked_add(trimmed.len())?;
        if !(trim_start..trimmed_end).contains(&clicked_index) {
            return None;
        }

        self::file_link_from_reference(trimmed)?;
        let raw_reference = raw.get(trim_start..)?.to_owned();
        Some(FileLinkExtraction {
            raw: raw_reference.clone(),
            raw_reference,
            clicked_index: clicked_index.checked_sub(trim_start)?,
            explicitness,
            wrap,
        })
    }
}

impl FileLinkExtraction {
    fn resolve_unquoted(&self, cwd: &str) -> Option<FileLink> {
        if !self.raw.chars().any(char::is_whitespace) {
            return self::resolve_first_file_link(self::file_link_candidates(&self.raw_reference), cwd, self.wrap);
        }

        let boundaries = self::whitespace_boundaries(&self.raw);

        let clicked_word_start = boundaries
            .partition_point(|boundary| *boundary <= self.clicked_index)
            .saturating_sub(1);
        let clicked_word_end = clicked_word_start.saturating_add(1);
        if let Some(resolved) =
            self::resolve_anchored_unquoted_reference(self, cwd, &boundaries, clicked_word_start, clicked_word_end)
        {
            return Some(resolved);
        }
        if let Some(resolved) = self::resolve_bare_unquoted_reference(self, cwd, &boundaries) {
            return Some(resolved);
        }

        if self.raw.starts_with('[') && self.raw.contains("](") && self::markdown_target(&self.raw).is_none() {
            return None;
        }
        self.resolve_single(cwd)
    }

    fn resolve_single(&self, cwd: &str) -> Option<FileLink> {
        let resolved = self::resolve_first_file_link(self::file_link_candidates(&self.raw_reference), cwd, self.wrap)?;
        if matches!(self.explicitness, FileLinkExplicitness::Implicit)
            && matches!(self::file_link_existence(&resolved.path), FileLinkExistence::Missing)
        {
            return None;
        }
        Some(resolved)
    }
}

fn whitespace_boundaries(raw: &str) -> Vec<usize> {
    let mut boundaries = vec![0];
    for (index, ch) in raw.char_indices().filter(|(_, ch)| ch.is_whitespace()) {
        boundaries.push(index.saturating_add(ch.len_utf8()));
    }
    boundaries.push(raw.len());
    boundaries
}

// Path markers keep candidate expansion linear in the visible reference; bare names use the pane cwd below because
// they have no lexical marker that can distinguish them from surrounding prose.
fn resolve_anchored_unquoted_reference(
    extraction: &FileLinkExtraction,
    cwd: &str,
    boundaries: &[usize],
    clicked_word_start: usize,
    clicked_word_end: usize,
) -> Option<FileLink> {
    let mut seen_candidates = HashSet::new();
    let mut longest_existing = None;
    for start_index in 0..=clicked_word_start {
        let Some(&start) = boundaries.get(start_index) else {
            continue;
        };
        let Some(&word_end) = boundaries.get(start_index.saturating_add(1)) else {
            continue;
        };
        let Some(word) = extraction.raw.get(start..word_end).map(str::trim) else {
            continue;
        };
        if !self::is_path_anchor_word(word) {
            continue;
        }
        for end_index in clicked_word_end..boundaries.len() {
            let Some(&end) = boundaries.get(end_index) else {
                continue;
            };
            let Some(candidate_raw) = extraction.raw.get(start..end).map(str::trim) else {
                continue;
            };
            for candidate in self::file_link_candidates(candidate_raw) {
                if !seen_candidates.insert(candidate.clone()) {
                    continue;
                }
                let Some(resolved) = candidate.resolve_with_existence(cwd) else {
                    continue;
                };
                if matches!(resolved.existence, FileLinkExistence::Missing) {
                    continue;
                }
                if longest_existing
                    .as_ref()
                    .is_none_or(|(length, _)| candidate_raw.len() > *length)
                {
                    longest_existing = Some((candidate_raw.len(), resolved.link));
                }
            }
        }
    }
    longest_existing.map(|(_, link)| link)
}

fn resolve_bare_unquoted_reference(
    extraction: &FileLinkExtraction,
    cwd: &str,
    boundaries: &[usize],
) -> Option<FileLink> {
    let entries = std::fs::read_dir(cwd).ok()?;
    let mut seen_candidates = HashSet::new();
    let mut longest_existing = None;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        for (match_start, _) in extraction.raw.match_indices(&name) {
            let Some(match_end) = match_start.checked_add(name.len()) else {
                continue;
            };
            if extraction.clicked_index < match_start
                || extraction.clicked_index >= match_end
                || !self::is_bare_name_match(&extraction.raw, match_start, match_end)
            {
                continue;
            }
            let start_index = boundaries
                .partition_point(|boundary| *boundary <= match_start)
                .saturating_sub(1);
            let end_index = boundaries.partition_point(|boundary| *boundary < match_end);
            let Some(&start) = boundaries.get(start_index) else {
                continue;
            };
            let Some(&end) = boundaries.get(end_index) else {
                continue;
            };
            let Some(candidate_raw) = extraction.raw.get(start..end).map(str::trim) else {
                continue;
            };
            for candidate in self::file_link_candidates(candidate_raw) {
                if !seen_candidates.insert(candidate.clone()) {
                    continue;
                }
                let Some(resolved) = candidate.resolve_with_existence(cwd) else {
                    continue;
                };
                if matches!(resolved.existence, FileLinkExistence::Missing) {
                    continue;
                }
                if longest_existing
                    .as_ref()
                    .is_none_or(|(length, _)| candidate_raw.len() > *length)
                {
                    longest_existing = Some((candidate_raw.len(), resolved.link));
                }
            }
        }
    }
    longest_existing.map(|(_, link)| link)
}

fn is_path_anchor_word(raw: &str) -> bool {
    let path = self::trim_reference(raw).0;
    !path.starts_with("http://")
        && !path.starts_with("https://")
        && !path.starts_with("file://")
        && (PATH_START_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) || path.contains('/'))
}

fn is_bare_name_match(raw: &str, start: usize, end: usize) -> bool {
    let before = raw.get(..start).and_then(|prefix| prefix.chars().next_back());
    let after = raw.get(end..).and_then(|suffix| suffix.chars().next());
    before.is_none_or(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'' | '`'))
        && after.is_none_or(|ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '/' | ':' | '.' | ',' | ';' | '!' | '?' | ')' | ']' | '}' | '>' | '"' | '\'' | '`'
                )
        })
}

fn resolve_first_file_link(candidates: Vec<FileLink>, cwd: &str, wrap: FileLinkWrap) -> Option<FileLink> {
    let mut fallback_plain = None;
    let mut fallback_with_location = None;
    let mut fallback_order = FileLinkFallbackOrder::PreservePath;
    for candidate in candidates {
        let Some(resolved) = candidate.resolve_with_existence(cwd) else {
            continue;
        };
        if matches!(wrap, FileLinkWrap::SoftWrapped) && matches!(resolved.existence, FileLinkExistence::Missing) {
            continue;
        }
        if matches!(resolved.existence, FileLinkExistence::Existing) {
            return Some(resolved.link);
        }
        if candidate.line.is_some() || candidate.column.is_some() {
            if candidate.line.is_some() && candidate.column.is_some() {
                fallback_order = FileLinkFallbackOrder::PreserveLocation;
            }
            fallback_with_location = Some(resolved.link);
        } else if fallback_plain.is_none() {
            fallback_plain = Some(resolved.link);
        }
    }
    // An unresolved raw path wins over a parsed location. This preserves valid names such as `report:2026` while
    // existing `report:2026` references still resolve as a location when `report` exists. Two numeric suffixes remain
    // an unambiguous `path:line:column` reference even when the target file is not present yet.
    match fallback_order {
        FileLinkFallbackOrder::PreservePath => fallback_plain.or(fallback_with_location),
        FileLinkFallbackOrder::PreserveLocation => fallback_with_location.or(fallback_plain),
    }
}

fn file_link_from_reference(raw: &str) -> Option<FileLink> {
    let (path, line, column) = self::parse_reference(raw)?;
    Some(FileLink {
        path: PathBuf::from(self::unescape_path(path)),
        line,
        column,
    })
}

fn file_link_candidates(raw: &str) -> Vec<FileLink> {
    let mut candidates = Vec::new();
    let mut add = |path: &str, line: Option<u32>, column: Option<u32>| {
        let candidate = FileLink {
            path: PathBuf::from(self::unescape_path(path)),
            line,
            column,
        };
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    };

    if let Some(path) = self::raw_path_for_resolution(raw)
        && !path.is_empty()
        && !path.starts_with("http://")
        && !path.starts_with("https://")
        && !path.starts_with("file://")
    {
        add(path, None, None);
    }
    if let Some((path, line, column)) = self::parse_reference_preserving_path_punctuation(raw) {
        add(path, line, column);
    }
    if let Some(link) = self::file_link_from_reference(raw)
        && !candidates.contains(&link)
    {
        candidates.push(link);
    }
    candidates
}

fn collect_left(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
    mode: FileLinkReferenceMode,
) -> ReferenceParts {
    let mut chars = Vec::new();
    let mut wrap = FileLinkWrap::SingleRow;
    let quoted_delimiter = self::quoted_reference_delimiter(frame_buffer, region, position);
    let inside_quote = quoted_delimiter.is_some();
    let origin_row = position.row;
    let mut current = self::previous_reference_position(frame_buffer, region, position);
    while let Some(position) = current {
        if position.row != origin_row
            && self::row_wrap(region, position.row.saturating_sub(1)) == Some(RowWrap::EndsWithSoftWrap)
        {
            wrap = FileLinkWrap::SoftWrapped;
        }
        let Some(text) = self::cell_text(frame_buffer.cell(position.row, position.col)) else {
            break;
        };
        if self::is_whitespace_text(text) {
            if self::is_continuation_row_prefix(frame_buffer, region, position) {
                wrap = FileLinkWrap::SoftWrapped;
                current = self::previous_row_end(region, position.row);
                continue;
            }
            if self::is_soft_wrap_row_suffix(frame_buffer, region, position) {
                wrap = FileLinkWrap::SoftWrapped;
                current = self::previous_reference_position(frame_buffer, region, position);
                continue;
            }
            if matches!(mode, FileLinkReferenceMode::UnquotedSpaces)
                && !self::row_prefix_is_whitespace(frame_buffer, region, position)
            {
                chars.push(text.to_owned());
                current = self::previous_reference_position(frame_buffer, region, position);
                continue;
            }
            if inside_quote || self::is_escaped_space(frame_buffer, region, position) {
                chars.push(text.to_owned());
                current = self::previous_reference_position(frame_buffer, region, position);
                continue;
            }
            break;
        }
        if !self::is_reference_text(text) {
            break;
        }
        chars.push(text.to_owned());
        if inside_quote && self::quote_text_matches(text, quoted_delimiter) {
            break;
        }
        current = self::previous_reference_position(frame_buffer, region, position);
    }
    ReferenceParts { chars, wrap }
}

fn collect_right(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
    mode: FileLinkReferenceMode,
) -> ReferenceParts {
    let mut chars = Vec::new();
    let mut wrap = FileLinkWrap::SingleRow;
    let quoted_delimiter = self::quoted_reference_delimiter(frame_buffer, region, position);
    let mut inside_quote = quoted_delimiter.is_some();
    let origin_row = position.row;
    let mut current = self::next_reference_position(frame_buffer, region, position);
    let mut skip_continuation_prefix = current.is_some_and(|next| next.row != position.row);
    while let Some(position) = current {
        if position.row != origin_row
            && self::row_wrap(region, position.row.saturating_sub(1)) == Some(RowWrap::EndsWithSoftWrap)
        {
            wrap = FileLinkWrap::SoftWrapped;
        }
        let Some(text) = self::cell_text(frame_buffer.cell(position.row, position.col)) else {
            break;
        };
        if self::is_whitespace_text(text) {
            if skip_continuation_prefix && self::row_prefix_is_whitespace(frame_buffer, region, position) {
                wrap = FileLinkWrap::SoftWrapped;
                let next = self::next_reference_position(frame_buffer, region, position);
                skip_continuation_prefix = next.is_some_and(|next| next.row != position.row);
                current = next;
                continue;
            }
            if self::is_soft_wrap_row_suffix(frame_buffer, region, position) {
                wrap = FileLinkWrap::SoftWrapped;
                let Some(next_row) = position.row.checked_add(1) else {
                    break;
                };
                current = Some(ClientMousePosition {
                    row: next_row,
                    col: region.col(),
                });
                skip_continuation_prefix = true;
                continue;
            }
            if matches!(mode, FileLinkReferenceMode::UnquotedSpaces)
                && !self::row_suffix_is_whitespace(frame_buffer, region, position)
            {
                chars.push(text.to_owned());
                let next = self::next_reference_position(frame_buffer, region, position);
                skip_continuation_prefix = next.is_some_and(|next| next.row != position.row);
                current = next;
                continue;
            }
            if inside_quote || self::is_escaped_space(frame_buffer, region, position) {
                chars.push(text.to_owned());
                let next = self::next_reference_position(frame_buffer, region, position);
                skip_continuation_prefix = next.is_some_and(|next| next.row != position.row);
                current = next;
                continue;
            }
            break;
        }
        skip_continuation_prefix = false;
        if !self::is_reference_text(text) {
            break;
        }
        chars.push(text.to_owned());
        if inside_quote && self::quote_text_matches(text, quoted_delimiter) {
            inside_quote = false;
        }
        let next = self::next_reference_position(frame_buffer, region, position);
        if let Some(next) = next
            && next.row != position.row
        {
            skip_continuation_prefix = true;
        }
        current = next;
    }
    ReferenceParts { chars, wrap }
}

fn previous_position(region: &PaneRegionSnapshot, position: ClientMousePosition) -> Option<ClientMousePosition> {
    if position.col > region.col() {
        return Some(ClientMousePosition {
            row: position.row,
            col: position.col.saturating_sub(1),
        });
    }
    if position.row <= region.row()
        || self::row_wrap(region, position.row.saturating_sub(1)) != Some(RowWrap::EndsWithSoftWrap)
    {
        return None;
    }
    self::previous_row_end(region, position.row)
}

fn previous_reference_position(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> Option<ClientMousePosition> {
    let mut position = self::previous_position(region, position);
    while let Some(current) = position
        && frame_buffer
            .cell(current.row, current.col)
            .is_some_and(|cell| cell.width() == RenderCellWidth::WideContinuation)
    {
        position = self::previous_position(region, current);
    }
    position
}

fn next_position(region: &PaneRegionSnapshot, position: ClientMousePosition) -> Option<ClientMousePosition> {
    let last_col = region.col().checked_add(region.cols())?.checked_sub(1)?;
    if position.col < last_col {
        return Some(ClientMousePosition {
            row: position.row,
            col: position.col.saturating_add(1),
        });
    }
    if position.row >= region.row().checked_add(region.rows())?.saturating_sub(1)
        || self::row_wrap(region, position.row) != Some(RowWrap::EndsWithSoftWrap)
    {
        return None;
    }
    Some(ClientMousePosition {
        row: position.row.saturating_add(1),
        col: region.col(),
    })
}

fn next_reference_position(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> Option<ClientMousePosition> {
    let mut position = self::next_position(region, position);
    while let Some(current) = position
        && frame_buffer
            .cell(current.row, current.col)
            .is_some_and(|cell| cell.width() == RenderCellWidth::WideContinuation)
    {
        position = self::next_position(region, current);
    }
    position
}

fn reference_position_for_click(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> Option<ClientMousePosition> {
    if frame_buffer
        .cell(position.row, position.col)
        .is_some_and(|cell| cell.width() == RenderCellWidth::WideContinuation)
    {
        let previous = self::previous_position(region, position)?;
        return frame_buffer
            .cell(previous.row, previous.col)
            .is_some_and(|cell| cell.width() == RenderCellWidth::Wide)
            .then_some(previous);
    }
    Some(position)
}

fn previous_row_end(region: &PaneRegionSnapshot, row: u16) -> Option<ClientMousePosition> {
    Some(ClientMousePosition {
        row: row.checked_sub(1)?,
        col: region.col().checked_add(region.cols())?.checked_sub(1)?,
    })
}

fn row_wrap(region: &PaneRegionSnapshot, row: u16) -> Option<RowWrap> {
    let local_row = row.checked_sub(region.row())?;
    let content_row = region.visible_top_row().checked_add(u64::from(local_row))?;
    Some(region.content_row_wrap(content_row))
}

fn is_continuation_row_prefix(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> bool {
    position.row > region.row()
        && self::row_wrap(region, position.row.saturating_sub(1)) == Some(RowWrap::EndsWithSoftWrap)
        && self::row_prefix_is_whitespace(frame_buffer, region, position)
}

fn is_soft_wrap_row_suffix(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> bool {
    self::row_wrap(region, position.row) == Some(RowWrap::EndsWithSoftWrap)
        && position.col >= region.col()
        && position.col < region.col().saturating_add(region.cols())
        && self::row_suffix_is_whitespace(frame_buffer, region, position)
}

fn row_prefix_is_whitespace(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> bool {
    (region.col()..=position.col)
        .all(|col| self::cell_text(frame_buffer.cell(position.row, col)).is_some_and(self::is_whitespace_text))
}

fn row_suffix_is_whitespace(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> bool {
    let Some(last_col) = region
        .col()
        .checked_add(region.cols())
        .and_then(|end| end.checked_sub(1))
    else {
        return false;
    };
    (position.col..=last_col)
        .all(|col| self::cell_text(frame_buffer.cell(position.row, col)).is_some_and(self::is_whitespace_text))
}

fn parse_reference(raw: &str) -> Option<(&str, Option<u32>, Option<u32>)> {
    let (raw, _) = self::trim_reference(raw);
    let raw = self::markdown_target(raw).unwrap_or(raw);
    let (raw, _) = self::trim_reference(raw);
    let mut path = raw;
    let mut line = None;
    let mut column = None;

    if let Some((before_column, raw_column)) = raw.rsplit_once(':')
        && let Some(parsed_column) = self::positive_number(raw_column)
    {
        if let Some((before_line, raw_line)) = before_column.rsplit_once(':')
            && let Some(parsed_line) = self::positive_number(raw_line)
        {
            path = before_line;
            line = Some(parsed_line);
            column = Some(parsed_column);
        } else {
            path = before_column;
            line = Some(parsed_column);
        }
    }

    path = self::trim_reference(path).0;

    if path.is_empty() || path.starts_with("http://") || path.starts_with("https://") || path.starts_with("file://") {
        return None;
    }

    Some((path, line, column))
}

fn parse_reference_preserving_path_punctuation(raw: &str) -> Option<(&str, Option<u32>, Option<u32>)> {
    let (_, start) = self::trim_reference(raw);
    let raw = raw.get(start..)?;
    let raw = self::markdown_target(raw).unwrap_or(raw);
    let mut path = raw;
    let mut line = None;
    let mut column = None;

    if let Some((before_column, raw_column)) = raw.rsplit_once(':')
        && let Some(parsed_column) = self::positive_number(self::trim_location_punctuation(raw_column))
    {
        if let Some((before_line, raw_line)) = before_column.rsplit_once(':')
            && let Some(parsed_line) = self::positive_number(self::trim_location_punctuation(raw_line))
        {
            path = before_line;
            line = Some(parsed_line);
            column = Some(parsed_column);
        } else {
            path = before_column;
            line = Some(parsed_column);
        }
    }

    if path.is_empty() || path.starts_with("http://") || path.starts_with("https://") || path.starts_with("file://") {
        return None;
    }
    Some((path, line, column))
}

fn raw_path_for_resolution(raw: &str) -> Option<&str> {
    let (_, start) = self::trim_reference(raw);
    let raw = raw.get(start..)?;
    let path = self::markdown_target(raw).unwrap_or(raw);
    (!path.is_empty()).then_some(path)
}

fn trim_location_punctuation(raw: &str) -> &str {
    raw.trim_end_matches(['.', ',', ';', '!', '?', ')', ']', '}', '>', '"', '\'', '`'])
}

fn trim_reference(raw: &str) -> (&str, usize) {
    let mut start = 0;
    let mut end = raw.len();
    while let Some(ch) = raw.get(start..).and_then(|rest| rest.chars().next())
        && matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'' | '`')
        && !(start == 0 && ch == '[' && raw.contains("]("))
    {
        start = start.saturating_add(ch.len_utf8());
    }
    while let Some(ch) = raw.get(..end).and_then(|rest| rest.chars().next_back())
        && matches!(ch, '.' | ',' | ';' | '!' | '?' | ']' | '}' | '>' | '"' | '\'' | '`')
    {
        end = end.saturating_sub(ch.len_utf8());
    }
    let mut trimmed = raw.get(start..end).unwrap_or_default();
    let mut unmatched_closing_parentheses = trimmed
        .matches(')')
        .count()
        .saturating_sub(trimmed.matches('(').count());
    while unmatched_closing_parentheses > 0 && trimmed.ends_with(')') {
        let Some(without_wrapper) = trimmed.strip_suffix(')') else {
            break;
        };
        trimmed = without_wrapper.trim_end_matches(['.', ',', ';', '!', '?', ')', ']', '}', '>', '"', '\'', '`']);
        unmatched_closing_parentheses = unmatched_closing_parentheses.saturating_sub(1);
    }
    (trimmed, start)
}

fn markdown_target(raw: &str) -> Option<&str> {
    let body = raw.strip_prefix('[')?;
    let (_, target) = body.split_once("](")?;
    let target = target.strip_suffix(')')?;
    Some(
        target
            .strip_prefix('<')
            .and_then(|target| target.strip_suffix('>'))
            .unwrap_or(target),
    )
}

fn resolve_path(path: &str, cwd: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    self::resolve_path_with_home(path, cwd, home.as_deref())
}

fn resolve_path_with_home(path: &str, cwd: &str, home: Option<&Path>) -> Option<PathBuf> {
    let expand_home = |path: &str| -> Option<PathBuf> {
        if path == "~" {
            Some(home?.to_owned())
        } else if let Some(rest) = path.strip_prefix("~/") {
            Some(home?.join(rest))
        } else {
            Some(PathBuf::from(path))
        }
    };
    let path = expand_home(path)?;
    if path.is_absolute() {
        return Some(path);
    }
    Some(expand_home(cwd)?.join(path))
}

fn is_bare_name(path: &str) -> bool {
    !PATH_START_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) && !path.contains('/')
}

fn file_link_existence(path: &Path) -> FileLinkExistence {
    match path.metadata() {
        Ok(metadata) if metadata.is_file() || metadata.is_dir() => FileLinkExistence::Existing,
        _ => FileLinkExistence::Missing,
    }
}

fn positive_number(raw: &str) -> Option<u32> {
    let value = raw.parse::<u32>().ok()?;
    (value > 0).then_some(value)
}

fn cell_text(cell: Option<&RenderCell>) -> Option<&str> {
    let text = cell?.text();
    (!text.is_empty() && text.chars().all(|ch| !ch.is_control())).then_some(text)
}

fn is_whitespace_text(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

fn is_reference_text(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| !ch.is_whitespace() && !ch.is_control())
}

fn quoted_reference_delimiter(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> Option<char> {
    let mut positions = Vec::new();
    let mut current = self::previous_reference_position(frame_buffer, region, position);
    while let Some(current_position) = current {
        positions.push(current_position);
        current = self::previous_reference_position(frame_buffer, region, current_position);
    }
    positions.reverse();

    let mut delimiter = None;
    let mut backslashes = 0_usize;
    for position in positions {
        let Some(text) = self::cell_text(frame_buffer.cell(position.row, position.col)) else {
            continue;
        };
        if text == "\\" {
            backslashes = backslashes.saturating_add(1);
            continue;
        }
        if let Some(quote) = self::quote_char(text)
            && backslashes.is_multiple_of(2)
            && !self::is_word_internal_quote(frame_buffer, region, position)
        {
            if delimiter == Some(quote) {
                delimiter = None;
            } else if delimiter.is_none() {
                delimiter = Some(quote);
            }
        }
        backslashes = 0;
    }
    delimiter
}

fn quote_char(text: &str) -> Option<char> {
    match text {
        "'" => Some('\''),
        "\"" => Some('"'),
        _ => None,
    }
}

fn quote_text_matches(text: &str, delimiter: Option<char>) -> bool {
    delimiter.is_some_and(|delimiter| quote_char(text) == Some(delimiter))
}

fn is_word_internal_quote(
    frame_buffer: &FrameBuffer,
    region: &PaneRegionSnapshot,
    position: ClientMousePosition,
) -> bool {
    let previous = self::previous_reference_position(frame_buffer, region, position)
        .and_then(|position| self::cell_text(frame_buffer.cell(position.row, position.col)));
    let next = self::next_reference_position(frame_buffer, region, position)
        .and_then(|position| self::cell_text(frame_buffer.cell(position.row, position.col)));
    previous.is_some_and(self::is_word_text) && next.is_some_and(self::is_word_text)
}

fn is_word_text(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
}

fn is_escaped_space(frame_buffer: &FrameBuffer, region: &PaneRegionSnapshot, position: ClientMousePosition) -> bool {
    let mut backslashes: usize = 0;
    let mut current = self::previous_reference_position(frame_buffer, region, position);
    while let Some(position) = current {
        if self::cell_text(frame_buffer.cell(position.row, position.col)) != Some("\\") {
            break;
        }
        backslashes = backslashes.saturating_add(1);
        current = self::previous_reference_position(frame_buffer, region, position);
    }
    backslashes % 2 == 1
}

fn unescape_path(raw: &str) -> String {
    let mut path = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek().is_some_and(|next| matches!(next, ' ' | '\\' | '\'' | '"')) {
            let _ = chars.next().map(|escaped| path.push(escaped));
        } else {
            path.push(ch);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use muxr_core::PaneId;
    use muxr_core::PaneMouseMode;
    use muxr_core::RenderBaseline;
    use muxr_core::RenderCell;
    use muxr_core::RenderCursor;
    use muxr_core::RenderCursorShape;
    use muxr_core::RenderRowSpan;
    use muxr_core::RenderStyle;
    use muxr_core::RowWrap;
    use muxr_core::TerminalSize;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_at_when_soft_wrap_has_continuation_indentation_returns_complete_reference() -> rootcause::Result<()> {
        let frame_buffer = self::frame_buffer(&["/tmp/foo", "  :42:7 "])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 8, 2, PaneMouseMode::None, 0)?
            .with_wrapped_rows(vec![RowWrap::EndsWithSoftWrap, RowWrap::EndsBeforeSoftWrap])?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 1, col: 4 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("/tmp/foo"),
                line: Some(42),
                column: Some(7),
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_soft_wrap_joins_adjacent_output_entries_rejects_joined_name() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let frame_buffer = self::frame_buffer(&["first", "second.rs"])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 10, 2, PaneMouseMode::None, 0)?
            .with_wrapped_rows(vec![RowWrap::EndsWithSoftWrap, RowWrap::EndsBeforeSoftWrap])?;

        let link = frame_buffer.file_link_at_resolved(&region, ClientMousePosition { row: 0, col: 2 }, cwd);

        assert_that!(link, none());
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_soft_wrap_has_indentation_returns_existing_joined_path() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("nested/firstsecond.rs");
        std::fs::create_dir_all(path.parent().expect("nested path has parent"))?;
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let frame_buffer = self::frame_buffer(&["nested/first", "  second.rs"])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 12, 2, PaneMouseMode::None, 0)?
            .with_wrapped_rows(vec![RowWrap::EndsWithSoftWrap, RowWrap::EndsBeforeSoftWrap])?;

        let link = frame_buffer.file_link_at_resolved(&region, ClientMousePosition { row: 1, col: 5 }, cwd);

        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_when_hard_row_boundary_ends_reference() -> rootcause::Result<()> {
        let frame_buffer = self::frame_buffer(&["/tmp/foo", ":42:7  "])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 8, 2, PaneMouseMode::None, 0)?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 0, col: 7 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("/tmp/foo"),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_parse_reference_when_location_and_punctuation_are_supplied_returns_trimmed_reference() {
        assert_eq!(
            parse_reference("../foo/baz:42:7,"),
            Some(("../foo/baz", Some(42), Some(7)))
        );
    }

    #[test]
    fn test_parse_reference_when_bare_file_name_is_supplied_returns_reference() {
        assert_eq!(parse_reference("README.md"), Some(("README.md", None, None)));
    }

    #[test]
    fn test_at_when_unicode_path_is_supplied_returns_reference() -> rootcause::Result<()> {
        let frame_buffer = self::frame_buffer(&["/tmp/über.rs"])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 12, 1, PaneMouseMode::None, 0)?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 0, col: 5 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("/tmp/über.rs"),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_when_wide_unicode_path_is_clicked_returns_reference() -> rootcause::Result<()> {
        let style = RenderStyle::default();
        let cells = vec![
            RenderCell::narrow("/", style),
            RenderCell::narrow("t", style),
            RenderCell::narrow("m", style),
            RenderCell::narrow("p", style),
            RenderCell::narrow("/", style),
            RenderCell::wide("界", style),
            RenderCell::wide_continuation(style),
            RenderCell::narrow(".", style),
            RenderCell::narrow("r", style),
            RenderCell::narrow("s", style),
        ];
        let frame_buffer = self::frame_buffer_from_cells(cells)?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 10, 1, PaneMouseMode::None, 0)?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 0, col: 6 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("/tmp/界.rs"),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_when_markdown_link_is_clicked_returns_target_reference() -> rootcause::Result<()> {
        let text = "[src/foo.rs](src/foo.rs)";
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 0, col: 5 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("src/foo.rs"),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_when_cell_contains_combining_character_preserves_cell_text() -> rootcause::Result<()> {
        let style = RenderStyle::default();
        let cells = vec![
            RenderCell::narrow("/", style),
            RenderCell::narrow("t", style),
            RenderCell::narrow("m", style),
            RenderCell::narrow("p", style),
            RenderCell::narrow("/", style),
            RenderCell::narrow("c", style),
            RenderCell::narrow("a", style),
            RenderCell::narrow("f", style),
            RenderCell::narrow("e\u{301}", style),
            RenderCell::narrow(".", style),
            RenderCell::narrow("r", style),
            RenderCell::narrow("s", style),
        ];
        let frame_buffer = self::frame_buffer_from_cells(cells)?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 12, 1, PaneMouseMode::None, 0)?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 0, col: 8 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("/tmp/cafe\u{301}.rs"),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_when_quoted_path_contains_spaces_returns_reference() -> rootcause::Result<()> {
        let text = r#""/tmp/a file.rs":42"#;
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 0, col: 9 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("/tmp/a file.rs"),
                line: Some(42),
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_when_quoted_path_soft_wrap_has_indentation_returns_trimmed_reference() -> rootcause::Result<()> {
        let frame_buffer = self::frame_buffer(&[r#""/tmp/a"#, "   file.rs\":42"])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 14, 2, PaneMouseMode::None, 0)?
            .with_wrapped_rows(vec![RowWrap::EndsWithSoftWrap, RowWrap::EndsBeforeSoftWrap])?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 1, col: 5 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("/tmp/afile.rs"),
                line: Some(42),
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_when_escaped_path_contains_spaces_returns_reference() -> rootcause::Result<()> {
        let text = r"/tmp/a\ file.rs";
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 15, 1, PaneMouseMode::None, 0)?;

        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 0, col: 8 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("/tmp/a file.rs"),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_when_click_is_between_quoted_references_does_not_join_them() -> rootcause::Result<()> {
        let text = r#""foo" text "bar.rs""#;
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;
        let link = frame_buffer.file_link_at(&region, ClientMousePosition { row: 0, col: 7 });
        assert_that!(
            link,
            some(eq(FileLink {
                path: PathBuf::from("text"),
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_bare_file_name_contains_spaces_uses_cwd() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("a file.rs");
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let text = "Open a file.rs now";
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at_resolved(
            &region,
            ClientMousePosition {
                row: 0,
                col: u16::try_from(text.find("file").expect("file name is present"))?,
            },
            cwd,
        );
        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_existing_path_contains_colon_and_trailing_punctuation_preserves_path()
    -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("name:with-colon!");
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let text = "name:with-colon!";
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 16, 1, PaneMouseMode::None, 0)?;

        let link = frame_buffer.file_link_at_resolved(&region, ClientMousePosition { row: 0, col: 6 }, cwd);

        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_nonexistent_path_contains_numeric_colon_preserves_path() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("report:2026");
        let text = path
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary path is not UTF-8"))?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary directory is not UTF-8"))?;
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        assert_that!(
            frame_buffer.file_link_at_resolved(&region, ClientMousePosition { row: 0, col: 3 }, cwd),
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_existing_path_precedes_numeric_colon_parses_location() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("report"), b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary directory is not UTF-8"))?;
        let text = "report:2026";
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(PaneId::new(1)?, 0, 0, 11, 1, PaneMouseMode::None, 0)?;

        assert_that!(
            frame_buffer.file_link_at_resolved(&region, ClientMousePosition { row: 0, col: 3 }, cwd),
            some(eq(FileLink {
                path: directory.path().join("report"),
                line: Some(2026),
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_explicit_path_with_spaces_is_followed_by_prose_uses_existing_path() -> rootcause::Result<()>
    {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("a file.rs");
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let text = format!("{} (line 1)", path.display());
        let frame_buffer = self::frame_buffer(&[&text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at_resolved(
            &region,
            ClientMousePosition {
                row: 0,
                col: u16::try_from(
                    text.find("file")
                        .ok_or_else(|| rootcause::report!("file name not found"))?,
                )?,
            },
            cwd,
        );
        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_existing_path_follows_prose_uses_path() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("existing.rs");
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let text = format!("Open {}", path.display());
        let frame_buffer = self::frame_buffer(&[&text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at_resolved(
            &region,
            ClientMousePosition {
                row: 0,
                col: u16::try_from(
                    text.find("existing")
                        .ok_or_else(|| rootcause::report!("file name not found"))?,
                )?,
            },
            cwd,
        );

        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_path_component_with_spaces_precedes_slash_uses_path() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("a directory/file.rs");
        let parent = path
            .parent()
            .ok_or_else(|| rootcause::report!("file path has no parent"))?;
        std::fs::create_dir_all(parent)?;
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let relative_path = path
            .strip_prefix(directory.path())
            .map_err(|error| rootcause::report!("failed to make test path relative: {error}"))?;
        let text = format!("Open {} now", relative_path.display());
        let frame_buffer = self::frame_buffer(&[&text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at_resolved(
            &region,
            ClientMousePosition {
                row: 0,
                col: u16::try_from(text.find("a directory").expect("path component is present"))?,
            },
            cwd,
        );

        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_long_path_follows_prose_uses_complete_path() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let filename = (0..65)
            .map(|index| format!("w{}", index % 10))
            .collect::<Vec<_>>()
            .join(" ");
        let path = directory.path().join(filename);
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let text = format!("Open {} now", path.display());
        let frame_buffer = self::frame_buffer(&[&text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at_resolved(
            &region,
            ClientMousePosition {
                row: 0,
                col: u16::try_from(
                    text.find("w6")
                        .ok_or_else(|| rootcause::report!("long path word is not present"))?,
                )?,
            },
            cwd,
        );

        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_backtick_wrapped_path_contains_spaces_trims_wrapper() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("a file.rs");
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let text = format!("`{}`", path.display());
        let frame_buffer = self::frame_buffer(&[&text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at_resolved(
            &region,
            ClientMousePosition {
                row: 0,
                col: u16::try_from(
                    text.find("file")
                        .ok_or_else(|| rootcause::report!("file name not found"))?,
                )?,
            },
            cwd,
        );

        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_unquoted_nonexistent_name_is_followed_by_prose_rejects_ambiguous_reference()
    -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let text = "new file.rs (not yet created)";
        let frame_buffer = self::frame_buffer(&[text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at_resolved(&region, ClientMousePosition { row: 0, col: 1 }, cwd);

        assert_that!(link, none());
        Ok(())
    }

    #[test]
    fn test_at_resolved_when_markdown_link_is_followed_by_prose_uses_existing_target() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("foo.rs");
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let text = format!("[file]({}) description", path.display());
        let frame_buffer = self::frame_buffer(&[&text])?;
        let region = PaneRegionSnapshot::new(
            PaneId::new(1)?,
            0,
            0,
            u16::try_from(text.chars().count())?,
            1,
            PaneMouseMode::None,
            0,
        )?;

        let link = frame_buffer.file_link_at_resolved(&region, ClientMousePosition { row: 0, col: 1 }, cwd);
        assert_that!(
            link,
            some(eq(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_file_link_resolve_from_cwd_when_bare_file_exists_uses_cwd() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("README");
        std::fs::write(&path, b"content")?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let link = FileLink {
            path: PathBuf::from("README"),
            line: None,
            column: None,
        };

        assert_that!(
            link.resolve_from_cwd(cwd),
            eq(Some(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_file_link_resolve_from_cwd_when_bare_directory_exists_uses_cwd() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("src");
        std::fs::create_dir(&path)?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let link = FileLink {
            path: PathBuf::from("src"),
            line: None,
            column: None,
        };

        assert_that!(
            link.resolve_from_cwd(cwd),
            eq(Some(FileLink {
                path,
                line: None,
                column: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn test_file_link_resolve_from_cwd_when_bare_non_file_word_is_supplied_rejects_it() -> rootcause::Result<()> {
        let directory = tempfile::tempdir()?;
        let cwd = directory
            .path()
            .to_str()
            .ok_or_else(|| rootcause::report!("temporary test directory is not UTF-8"))?;
        let link = FileLink {
            path: PathBuf::from("status"),
            line: None,
            column: None,
        };

        assert_that!(link.resolve_from_cwd(cwd), none());
        Ok(())
    }

    #[test]
    fn test_file_link_resolve_from_cwd_when_path_is_relative_returns_absolute_path() {
        let link = FileLink {
            path: PathBuf::from("../new-file.rs"),
            line: Some(3),
            column: Some(4),
        };
        assert_eq!(
            link.resolve_from_cwd("/tmp/project"),
            Some(FileLink {
                path: PathBuf::from("/tmp/project/../new-file.rs"),
                line: Some(3),
                column: Some(4),
            })
        );
    }

    fn frame_buffer(lines: &[&str]) -> rootcause::Result<FrameBuffer> {
        let cols = lines.iter().map(|line| line.chars().count()).max().unwrap_or(1);
        let size = TerminalSize::new(u16::try_from(cols)?, u16::try_from(lines.len())?)?;
        let rows = lines
            .iter()
            .enumerate()
            .map(|(row, line)| {
                let mut cells = line
                    .chars()
                    .map(|ch| RenderCell::narrow(ch.to_string(), RenderStyle::default()))
                    .collect::<Vec<_>>();
                while cells.len() < cols {
                    cells.push(RenderCell::narrow(" ", RenderStyle::default()));
                }
                RenderRowSpan::new(u16::try_from(row)?, 0, cells)
            })
            .collect::<rootcause::Result<Vec<_>>>()?;
        self::frame_buffer_from_rows(size, rows)
    }

    fn frame_buffer_from_cells(cells: Vec<RenderCell>) -> rootcause::Result<FrameBuffer> {
        let size = TerminalSize::new(u16::try_from(cells.len())?, 1)?;
        let rows = vec![RenderRowSpan::new(0, 0, cells)?];
        self::frame_buffer_from_rows(size, rows)
    }

    fn frame_buffer_from_rows(size: TerminalSize, rows: Vec<RenderRowSpan>) -> rootcause::Result<FrameBuffer> {
        let update = muxr_core::RenderUpdate::Baseline(RenderBaseline::new(
            1,
            size,
            RenderCursor {
                row: 0,
                col: 0,
                shape: RenderCursorShape::Default,
                visibility: muxr_core::RenderCursorVisibility::Visible,
            },
            rows,
        )?);
        let mut frame_buffer = FrameBuffer::default();
        frame_buffer.apply(update)?;
        Ok(frame_buffer)
    }
}
