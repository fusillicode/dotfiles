use std::convert::identity;

use nvim_oxi::Dictionary;
use nvim_oxi::Function;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;

use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::DiagnosticsFilters;
use crate::diagnostics::filters::buffer::BufferFilter;

pub fn filter() -> Object {
    Object::from(Function::<Vec<Dictionary>, _>::from_fn(filter_core))
}

fn filter_core(lsp_diags: Vec<Dictionary>) -> Vec<Dictionary> {
    let cur_buf = Buffer::current();
    let Ok(buf_path) = cur_buf
        .get_name()
        .map(|s| s.to_string_lossy().to_string())
        .inspect_err(|error| {
            crate::oxi_utils::notify_error(&format!(
                "can't get buffer name of buffer #{cur_buf:#?}, error {error:#?}"
            ));
        })
    else {
        return vec![];
    };

    // Keeping this as a separate filter because it short circuits the whole filtering and
    // doesn't require any LSP diagnostics to apply its logic.
    if BufferFilter::new()
        .skip_diagnostic(&buf_path, None)
        .inspect_err(|error| crate::oxi_utils::notify_error(&format!("error filtering by buffer {buf_path:#?}, error {error:#?}")))
        .is_ok_and(identity)
    {
        return vec![];
    };

    let Ok(filters) = DiagnosticsFilters::all(&lsp_diags)
        .inspect_err(|error| crate::oxi_utils::notify_error(&format!("can't get diangnostics filters, error {error:#?}")))
    else {
        return vec![];
    };

    let mut out = vec![];
    for lsp_diag in lsp_diags {
        if filters
            .skip_diagnostic(&buf_path, Some(&lsp_diag))
            .inspect_err(|error| {
                crate::oxi_utils::notify_error(&format!(
                    "error filtering dignostic {lsp_diag:#?} for buffer {buf_path:#?}, error {error:?}"
                ))
            })
            .is_ok_and(identity)
        {
            continue;
        }
        out.push(lsp_diag.clone());
    }
    out
}
