//! Shared resolution and source-edit support for the direct fixes.

pub(super) use fn_call_analysis::calls_in_scope;
pub(super) use fn_path_resolution::FnCallSuggestion;
pub(super) use fn_path_resolution::expected_fn_path;
pub(super) use import_cleanup::remove_unused_fn_imports;
pub(super) use import_edits::ImportEdits;
pub(super) use import_edits::LocalNames;
pub(super) use import_edits::local_names;
pub(super) use import_resolution::has_name_clash_parts;
pub(super) use module_idx::module_idx;
pub(super) use path_resolution::is_import_style_path;

mod fn_call_analysis;
mod fn_path_resolution;
mod import_cleanup;
mod import_edits;
pub(super) mod import_resolution;
pub(super) mod module_idx;
pub(super) mod path_resolution;
mod scope_bindings;
