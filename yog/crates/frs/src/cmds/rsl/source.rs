//! Edits against one parsed source snapshot. Inner replacements precede enclosing replacements.

use std::fmt::Display;
use std::ops::Range;

use proc_macro2::LineColumn;
use proc_macro2::Span;
use rootcause::report;

pub(super) mod item_ranges;

pub(super) struct SourceEditor<'source> {
    source: &'source str,
    rule: &'static str,
    edits: Vec<Edit>,
    skips: Vec<String>,
}

impl<'source> SourceEditor<'source> {
    pub(super) const fn new(source: &'source str, rule: &'static str) -> Self {
        Self {
            source,
            rule,
            edits: Vec::new(),
            skips: Vec::new(),
        }
    }

    pub(super) fn offset(&self, position: LineColumn) -> usize {
        item_ranges::byte_offset(self.source, position)
    }

    pub(super) fn span_range(&self, span: Span) -> Range<usize> {
        self.offset(span.start())..self.offset(span.end())
    }

    pub(super) fn replace_span(&mut self, span: Span, replacement: impl Into<String>) -> rootcause::Result<()> {
        self.replace(self.span_range(span), replacement.into())
    }

    pub(super) fn can_replace_path(&mut self, span: Span) -> bool {
        let text = self.source.get(self.span_range(span)).unwrap_or_default();

        if text.contains("/*") || text.contains("//") {
            self.skip("path contains comments");
            false
        } else {
            true
        }
    }

    pub(super) fn replace(&mut self, range: Range<usize>, replacement: String) -> rootcause::Result<()> {
        if self.source.get(range.clone()).is_none() {
            return Err(report!("invalid source edit range"));
        }

        if let Some(edit) = self
            .edits
            .iter_mut()
            .find(|edit| edit.range == range && range.is_empty())
        {
            edit.replacement.push_str(&replacement);
            return Ok(());
        }

        if self.edits.iter().any(|edit| {
            let overlaps = edit.range.start < range.end && range.start < edit.range.end;
            let contained = range.start <= edit.range.start && edit.range.end <= range.end;
            overlaps && !contained
        }) {
            return Err(report!("overlapping source edits"));
        }

        self.edits
            .retain(|edit| !(range.start <= edit.range.start && edit.range.end <= range.end));
        self.edits.push(Edit { range, replacement });
        self.edits
            .sort_by_key(|edit| (edit.range.start, !edit.range.is_empty()));

        Ok(())
    }

    pub(super) fn render_range(&self, range: Range<usize>) -> rootcause::Result<String> {
        let mut result = String::new();
        let mut cursor = range.start;

        for edit in &self.edits {
            if edit.range.start < range.start || edit.range.end > range.end {
                continue;
            }

            let unchanged = self
                .source
                .get(cursor..edit.range.start)
                .ok_or_else(|| report!("invalid source edit boundary"))?;
            result.push_str(unchanged);
            result.push_str(&edit.replacement);
            cursor = edit.range.end;
        }

        let remaining = self
            .source
            .get(cursor..range.end)
            .ok_or_else(|| report!("invalid source edit boundary"))?;
        result.push_str(remaining);

        Ok(result)
    }

    pub(super) fn skip(&mut self, reason: impl Display) {
        self.skips.push(format!("skipped {}: {reason}", self.rule));
    }

    pub(super) fn finish(self) -> rootcause::Result<(String, Vec<String>)> {
        let source = self.render_range(0..self.source.len())?;

        Ok((source, self.skips))
    }
}

struct Edit {
    range: Range<usize>,
    replacement: String,
}
