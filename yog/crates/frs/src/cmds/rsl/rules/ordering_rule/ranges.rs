//! Original-snapshot ranges. Gaps remain in their slots during reordering.

use proc_macro2::LineColumn;
use proc_macro2::Span;

#[derive(Clone, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct SourceRange {
    pub start: Position,
    pub end: Position,
    pub bytes: std::ops::Range<usize>,
    pub whole_lines: bool,
}

impl SourceRange {
    pub fn compact(&self) -> String {
        if self.whole_lines {
            format!("{}-{}", self.start.line, self.end.line)
        } else {
            format!(
                "{}:{}-{}:{}",
                self.start.line, self.start.column, self.end.line, self.end.column
            )
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

pub(super) fn item_ranges(source: &str, spans: &[Span], scope_start: LineColumn) -> Vec<SourceRange> {
    let raw: Vec<_> = spans
        .iter()
        .map(|span| byte_offset(source, span.start())..byte_offset(source, span.end()))
        .collect();

    let mut previous_end = None;
    raw.iter()
        .enumerate()
        .map(|(index, range)| {
            let lower = previous_end;
            let upper = raw.get(index.saturating_add(1)).map_or(source.len(), |next| next.start);
            let start = leading_comments(source, lower, range.start, byte_offset(source, scope_start));
            let end = trailing_comment(source, range.end, upper);

            let line_start = source
                .get(..start)
                .and_then(|prefix| prefix.rfind('\n'))
                .map_or(0, |offset| offset.saturating_add(1));
            let line_end = source
                .get(end..)
                .and_then(|suffix| suffix.find('\n'))
                .map_or(source.len(), |offset| end.saturating_add(offset));
            let whole_lines = source.get(line_start..start).is_some_and(|text| text.trim().is_empty())
                && source.get(end..line_end).is_some_and(|text| text.trim().is_empty());

            let bytes = if whole_lines { line_start..line_end } else { start..end };
            previous_end = Some(bytes.end);

            SourceRange {
                start: position_at(source, bytes.start),
                end: position_at(source, bytes.end),
                bytes,
                whole_lines,
            }
        })
        .collect()
}

fn byte_offset(source: &str, position: LineColumn) -> usize {
    let line_start = source
        .split_inclusive('\n')
        .take(position.line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    let line = source
        .get(line_start..)
        .unwrap_or_default()
        .split('\n')
        .next()
        .unwrap_or_default();
    let column = line
        .char_indices()
        .nth(position.column)
        .map_or(line.len(), |(offset, _)| offset);
    line_start.saturating_add(column)
}

fn position_at(source: &str, offset: usize) -> Position {
    let prefix = source.get(..offset).unwrap_or_default();
    let line = prefix.bytes().filter(|&byte| byte == b'\n').count().saturating_add(1);
    let column = prefix.rsplit('\n').next().unwrap_or_default().chars().count();

    Position {
        line,
        column: column.saturating_add(1),
    }
}

fn leading_comments(source: &str, lower: Option<usize>, start: usize, scope_start: usize) -> usize {
    let boundary = lower.unwrap_or(scope_start);
    let Some(gap) = source.get(boundary..start) else {
        return start;
    };

    // A same-line trailing comment belongs to the preceding item. Inner docs remain scope-owned.
    let skipped = if lower.is_some() {
        let Some(newline) = gap.find('\n') else {
            return start;
        };
        newline.saturating_add(1)
    } else {
        0
    };

    let mut cursor = skipped;
    let mut attached = None;
    let mut newlines = 0_usize;
    while let Some(rest) = gap.get(cursor..)
        && !rest.is_empty()
    {
        let Some(character) = rest.chars().next() else {
            break;
        };

        if character.is_whitespace() {
            if character == '\n' {
                newlines = newlines.saturating_add(1);
            }
            if newlines > 1 {
                attached = None;
            }
            cursor = cursor.saturating_add(character.len_utf8());
            continue;
        }

        let length = comment_length(rest);
        if let Some(length) = length {
            if rest.starts_with("//!") || rest.starts_with("/*!") {
                attached = None;
            } else {
                attached.get_or_insert_with(|| boundary.saturating_add(cursor));
            }
            cursor = cursor.saturating_add(length);
        } else {
            // Scope attributes and shebangs are scope-owned, including same-line comments.
            attached = None;
            cursor = cursor.saturating_add(rest.find('\n').unwrap_or(rest.len()));
        }

        newlines = 0;
    }

    attached.unwrap_or(start)
}

fn trailing_comment(source: &str, end: usize, upper: usize) -> usize {
    let gap = source.get(end..upper).unwrap_or_default();
    let mut cursor = 0_usize;
    loop {
        let rest = gap.get(cursor..).unwrap_or_default();
        let comment = rest
            .trim_start_matches(|character: char| character.is_whitespace() && character != '\n' && character != '\r');
        let spaces = rest.len().saturating_sub(comment.len());
        let Some(length) = comment_length(comment) else {
            break;
        };
        cursor = cursor.saturating_add(spaces).saturating_add(length);
    }
    end.saturating_add(cursor)
}

fn comment_length(text: &str) -> Option<usize> {
    if text.starts_with("//") {
        Some(text.find('\n').unwrap_or(text.len()))
    } else if text.starts_with("/*") {
        block_comment_end(text)
    } else {
        None
    }
}

fn block_comment_end(comment: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut characters = comment.char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        match (character, characters.peek()) {
            ('/', Some((_, '*'))) => {
                depth = depth.saturating_add(1);
                characters.next();
            }
            ('*', Some((_, '/'))) => {
                depth = depth.saturating_sub(1);
                characters.next();
                if depth == 0 {
                    return Some(offset.saturating_add(2));
                }
            }
            _ => {}
        }
    }
    None
}
