//! Shared name resolution and supporting edit plans for rule analysis.

pub(super) use fn_call_analysis::analyze_calls;
pub(super) use fn_path_resolution::expected_fn_path;
pub(super) use import_cleanup::plan_import_removals;
pub(super) use import_cleanup::removable_imports;
pub(super) use import_edits::ImportEdits;
pub(super) use import_edits::ImportError;
pub(super) use import_edits::local_names;
pub(super) use import_resolution::has_name_clash_parts;
pub(super) use module_idx::module_idx;
pub(super) use path_resolution::is_import_style_path;
use proc_macro2::Span;

use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::source::SourceEditor;

pub(super) mod fn_call_analysis;
pub(super) mod fn_path_resolution;
mod import_cleanup;
mod import_edits;
pub(super) mod import_resolution;
pub(super) mod module_idx;
pub(super) mod path_resolution;

pub(super) fn ensure_no_comments(edits: &SourceEditor<'_>, span: Span) -> Result<(), FixBlockReason> {
    let text = edits.source_range(edits.span_range(span));

    if text.contains("/*") || text.contains("//") {
        Err(comments_reason())
    } else {
        Ok(())
    }
}

pub(super) fn comments_reason() -> FixBlockReason {
    FixBlockReason {
        code: "comments",
        message: "The change could remove comments inside the affected code.".to_owned(),
        help: "Move or preserve the comments before making this change manually.",
    }
}
