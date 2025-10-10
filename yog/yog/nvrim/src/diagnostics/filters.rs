use nvim_oxi::Dictionary;

use crate::diagnostics::filters::related_info::RelatedInfoFilter;

pub mod buffer;
pub mod msg_blacklist;
pub mod related_info;

/// Trait for filtering diagnostics.
pub trait DiagnosticsFilter {
    /// Returns true if the diagnostic should be skipped.
    ///
    /// # Errors
    /// - Access to required diagnostic fields (dictionary keys) fails (missing key or wrong type).
    /// - Filter-specific logic (e.g. related info extraction) fails.
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool>;
}

/// A collection of diagnostic filters.
pub struct DiagnosticsFilters(Vec<Box<dyn DiagnosticsFilter>>);

impl DiagnosticsFilters {
    /// Creates all available diagnostic filters. The order of filters is IMPORTANT.
    ///
    /// # Errors
    /// - Constructing the related info filter fails (dictionary traversal or type mismatch).
    pub fn all(lsp_diags: &[Dictionary]) -> color_eyre::Result<Self> {
        let mut filters = msg_blacklist::typos_filters();
        filters.extend(msg_blacklist::harper_filters());
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
    fn skip_diagnostic(&self, buf_path: &str, lsp_diag: Option<&Dictionary>) -> color_eyre::Result<bool> {
        // The first filter that returns true skips the LSP diagnostic and all subsequent filters
        // evaluation.
        for filter in &self.0 {
            if filter.skip_diagnostic(buf_path, lsp_diag)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
