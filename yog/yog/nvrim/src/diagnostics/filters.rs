//! Collection and construction of diagnostic filters.
//!
//! Defines [`DiagnosticsFilter`] trait plus ordered creation of all active filters (message blacklist,
//! source‑specific sets, related info deduper). Ordering is significant for short‑circuit behavior.

use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::GetTextOpts;
use ytil_nvim_oxi::buffer::BufferExt;
use ytil_nvim_oxi::dict::DictionaryExt as _;

use crate::diagnostics::filters::related_info::RelatedInfoFilter;

pub mod buffer;
pub mod msg_blacklist;
pub mod related_info;

pub struct BufferWithPath {
    buffer: Box<dyn BufferExt>,
    path: String,
}

impl BufferWithPath {
    pub fn get_diagnosed_word(&self, lsp_diag: &Dictionary) -> color_eyre::Result<Option<String>> {
        // Error if these are missing. LSPs diagnostics seems to always have these fields.
        let lnum = lsp_diag.get_t::<nvim_oxi::Integer>("lnum")? as usize;
        let col = lsp_diag.get_t::<nvim_oxi::Integer>("col")? as usize;
        let end_col = lsp_diag.get_t::<nvim_oxi::Integer>("end_col")? as usize;
        let end_lnum = lsp_diag.get_t::<nvim_oxi::Integer>("end_lnum")? as usize;

        if lnum > end_lnum || col > end_col {
            return Ok(None);
        }

        let lines = self
            .buffer
            .get_text_as_vec_of_lines((lnum, col), (end_lnum, end_col), &GetTextOpts::default())?;

        let lines_len = lines.len();
        if lines_len == 0 {
            return Ok(None);
        }
        let last_line_idx = lines_len.saturating_sub(1);
        let adjusted_end_col = end_col.saturating_sub(col);

        let mut out = String::new();
        for (line_idx, line) in lines.iter().enumerate() {
            let line = line.to_string();
            let text = if line_idx == last_line_idx {
                line.get(..adjusted_end_col).unwrap_or(&line)
            } else {
                &line
            };
            out.push_str(text)
        }

        if out.is_empty() {
            return Ok(None);
        }

        Ok(Some(out))
    }
}

impl TryFrom<Buffer> for BufferWithPath {
    type Error = color_eyre::eyre::Error;

    fn try_from(value: Buffer) -> Result<Self, Self::Error> {
        let path = value.get_name().map(|s| s.to_string_lossy().to_string())?;
        Ok(Self {
            path,
            buffer: Box::new(value),
        })
    }
}

/// Trait for filtering diagnostics.
pub trait DiagnosticsFilter {
    /// Returns true if the diagnostic should be skipped.
    ///
    /// # Errors
    /// - Access to required diagnostic fields (dictionary keys) fails (missing key or wrong type).
    /// - Filter-specific logic (e.g. related info extraction) fails.
    fn skip_diagnostic(&self, buf: Option<&BufferWithPath>, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool>;
}

/// A collection of diagnostic filters.
pub struct DiagnosticsFilters(Vec<Box<dyn DiagnosticsFilter>>);

impl DiagnosticsFilters {
    /// Creates all available diagnostic filters. The order of filters is IMPORTANT.
    ///
    /// # Errors
    /// - Constructing the related info filter fails (dictionary traversal or type mismatch).
    pub fn all(lsp_diags: &[Dictionary]) -> color_eyre::Result<Self> {
        let mut filters = msg_blacklist::typos::filters();
        filters.extend(msg_blacklist::harper::filters());
        filters.push(Box::new(RelatedInfoFilter::new(lsp_diags)?));
        Ok(Self(filters))
    }
}

/// Implementation of [`DiagnosticsFilter`] for [`DiagnosticsFilters`].
impl DiagnosticsFilter for DiagnosticsFilters {
    /// Returns true if any filter skips the diagnostic.
    ///
    /// # Errors
    /// - A filter implementation (invoked in sequence) returns an error; it is propagated unchanged.
    fn skip_diagnostic(&self, buf: Option<&BufferWithPath>, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool> {
        // The first filter that returns true skips the LSP diagnostic and all subsequent filters
        // evaluation.
        for filter in &self.0 {
            if filter.skip_diagnostic(buf, lsp_diag)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use nvim_oxi::api::opts::GetTextOpts;
    use rstest::rstest;
    use ytil_nvim_oxi::buffer::BufferExt;

    use super::*;

    #[rstest]
    #[case::lnum_greater_than_end_lnum(
        vec!["hello world".to_string()],
        create_diag(1, 0, 0, 5),
        None
    )]
    #[case::col_greater_than_end_col(
        vec!["hello world".to_string()],
        create_diag(0, 5, 0, 0),
        None
    )]
    #[case::empty_lines(
        vec![],
        create_diag(0, 0, 0, 5),
        None
    )]
    #[case::single_line_partial_word(
        vec!["hello world".to_string()],
        create_diag(0, 0, 0, 5),
        Some("hello".to_string())
    )]
    #[case::single_line_full_word(
        vec!["hello".to_string()],
        create_diag(0, 0, 0, 5),
        Some("hello".to_string())
    )]
    #[case::multi_line_word(
        vec!["heal".to_string(), "lo".to_string()],
        create_diag(0, 0, 1, 2),
        Some("heallo".to_string())
    )]
    #[case::multi_line_partial_last_line(
        vec!["heal".to_string(), "lo world".to_string()],
        create_diag(0, 0, 1, 2),
        Some("heallo".to_string())
    )]
    #[case::start_col_out_of_bounds(
        vec!["hi".to_string()],
        create_diag(0, 10, 0, 15),
        None
    )]
    #[case::end_col_beyond_line(
        vec!["hi".to_string()],
        create_diag(0, 0, 0, 10),
        Some("hi".to_string())
    )]
    fn get_diagnosed_word_returns_expected(
        #[case] lines: Vec<String>,
        #[case] diag: Dictionary,
        #[case] expected: Option<String>,
    ) {
        let buf = create_buffer_with_path(lines);
        assert2::let_assert!(Ok(actual) = buf.get_diagnosed_word(&diag));
        pretty_assertions::assert_eq!(actual, expected);
    }

    struct MockBuffer(Vec<String>);

    impl BufferExt for MockBuffer {
        fn get_line(&self, _idx: usize) -> color_eyre::Result<nvim_oxi::String> {
            unimplemented!()
        }

        fn set_text_at_cursor_pos(&mut self, _text: &str) {
            unimplemented!()
        }

        fn get_text_as_vec_of_lines(
            &self,
            (start_lnum, start_col): (usize, usize),
            (end_lnum, end_col): (usize, usize),
            _opts: &GetTextOpts,
        ) -> Result<Vec<String>, nvim_oxi::api::Error> {
            if start_lnum > end_lnum || (start_lnum == end_lnum && start_col > end_col) {
                return Ok(vec![]);
            }
            let mut result = Vec::new();
            for lnum in start_lnum..=end_lnum {
                if lnum >= self.0.len() {
                    break;
                }
                let line = &self.0[lnum];
                let start = if lnum == start_lnum { start_col } else { 0 };
                let end = if lnum == end_lnum { end_col } else { line.len() };
                if start >= line.len() {
                    result.push(String::new());
                } else {
                    result.push(line[start..end.min(line.len())].to_string());
                }
            }
            Ok(result)
        }
    }

    fn create_diag(lnum: i64, col: i64, end_lnum: i64, end_col: i64) -> Dictionary {
        ytil_nvim_oxi::dict! { col: col, end_col: end_col, lnum: lnum, end_lnum: end_lnum }
    }

    fn create_buffer_with_path(lines: Vec<String>) -> BufferWithPath {
        BufferWithPath {
            buffer: Box::new(MockBuffer(lines)),
            path: "test.rs".to_string(),
        }
    }
}
