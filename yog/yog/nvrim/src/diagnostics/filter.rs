//! High-level diagnostic filtering pipeline.
//!
//! Orchestrates buffer path filtering, message blacklist, and related info deduplication, returning
//! retained diagnostics for display while reporting errors via notifications.

use std::convert::identity;

use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;

use crate::diagnostics::filters::BufferWithPath;
use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::DiagnosticsFilters;
use crate::diagnostics::filters::buffer::BufferFilter;
use crate::diagnostics::filters::buffer::BufferFilterImpl;

/// Filters LSP diagnostics based on configured filters.
pub fn filter(lsp_diags: Vec<Dictionary>) -> Vec<Dictionary> {
    let current_buffer = Buffer::current();

    let Ok(buffer_with_path) = BufferWithPath::try_from(current_buffer).inspect_err(|err| {
        ytil_noxi::notify::error(format!("error creating BufferWithContent | error={err:#?}"));
    }) else {
        return vec![];
    };

    // Keeping this as a separate filter because it short circuits the whole filtering and
    // does not require any LSP diagnostics to apply its logic, just the [`nvim_oxi::api::Buffer`].
    let buffer_filter = BufferFilterImpl;
    if buffer_filter
        .skip_diagnostic(&buffer_with_path)
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!("error getting filter by buffer | error={err:#?}"));
        })
        .unwrap_or(false)
    {
        return vec![];
    }

    let Ok(filters) = DiagnosticsFilters::all(&lsp_diags).inspect_err(|err| {
        ytil_noxi::notify::error(format!("error getting diagnostics filters | error={err:#?}"));
    }) else {
        return vec![];
    };

    let mut out = vec![];
    for lsp_diag in lsp_diags {
        if filters
            .skip_diagnostic(&buffer_with_path, &lsp_diag)
            .inspect_err(|err| {
                ytil_noxi::notify::error(format!(
                    "error filtering diagnostic | buffer={:?} diagnostic={lsp_diag:#?} error={err:#?}",
                    buffer_with_path.path()
                ));
            })
            .is_ok_and(identity)
        {
            continue;
        }
        out.push(lsp_diag.clone());
    }
    out
}
