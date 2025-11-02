//! Collection and construction of diagnostic filters.
//!
//! Defines [`DiagnosticsFilter`] trait plus ordered creation of all active filters (message blacklist,
//! source‑specific sets, related info deduper). Ordering is significant for short‑circuit behavior.
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::GetTextOpts;
use ytil_nvim_oxi::dict::DictionaryExt as _;

use crate::diagnostics::filters::related_info::RelatedInfoFilter;

pub mod buffer;
pub mod msg_blacklist;
pub mod related_info;

pub struct BufferWithPath {
    #[allow(dead_code)]
    buffer: Buffer,
    path: String,
}

impl BufferWithPath {
    #[allow(dead_code)]
    pub fn get_diagnosed_word(&self, lsp_diag: &Dictionary) -> color_eyre::Result<Option<String>> {
        // Error if these are missing. LSPs diagnostics seems to always have these fields.
        let col = lsp_diag.get_t::<nvim_oxi::Integer>("col")? as usize;
        let end_col = lsp_diag.get_t::<nvim_oxi::Integer>("end_col")? as usize;
        let lnum = lsp_diag.get_t::<nvim_oxi::Integer>("lnum")? as usize;
        let end_lnum = lsp_diag.get_t::<nvim_oxi::Integer>("end_lnum")? as usize;

        if lnum > end_lnum || col > end_col {
            return Ok(None);
        }

        let lines = self
            .buffer
            .get_text(lnum..end_lnum, col, end_col, &GetTextOpts::default())?
            .collect::<Vec<_>>();

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

        Ok(Some(out))
    }
}

impl TryFrom<Buffer> for BufferWithPath {
    type Error = color_eyre::eyre::Error;

    fn try_from(value: Buffer) -> Result<Self, Self::Error> {
        let path = value.get_name().map(|s| s.to_string_lossy().to_string())?;
        Ok(Self { path, buffer: value })
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
