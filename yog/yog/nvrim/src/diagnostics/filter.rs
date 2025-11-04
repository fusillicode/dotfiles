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

/// Filters LSP diagnostics based on configured filters.
pub fn filter(lsp_diags: Vec<Dictionary>) -> Vec<Dictionary> {
    let cur_buf = Buffer::current();
    let Ok(buf_path) = cur_buf
        .get_name()
        .map(|s| s.to_string_lossy().to_string())
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(format!("cannot get buffer name | buffer={cur_buf:#?} error={error:#?}"));
        })
    else {
        return vec![];
    };

    let Ok(buf_with_path) = BufferWithPath::try_from(cur_buf).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot create BufferWithContent | error={error:#?}"));
    }) else {
        return vec![];
    };

    // Keeping this as a separate filter because it short circuits the whole filtering and
    // does not require any LSP diagnostics to apply its logic.
    if BufferFilter::new().skip_diagnostic(&buf_with_path) {
        return vec![];
    }

    let Ok(filters) = DiagnosticsFilters::all(&lsp_diags).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get diagnostics filters | error={error:#?}"));
    }) else {
        return vec![];
    };

    let mut out = vec![];
    for lsp_diag in lsp_diags {
        if filters
            .skip_diagnostic(&buf_with_path, &lsp_diag)
            .inspect_err(|error| {
                ytil_nvim_oxi::api::notify_error(format!(
                    "cannot filter diagnostic | diagnostic={lsp_diag:#?} buffer={buf_path:#?} error={error:?}"
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
