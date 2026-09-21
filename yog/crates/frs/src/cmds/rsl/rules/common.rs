//! Shared analysis for frs rsl rules.

pub(super) use fn_call_analysis::CallDetails;
pub(super) use fn_call_analysis::FnCallFinding;
pub(super) use fn_call_analysis::FnCallKind;
pub(super) use fn_call_analysis::find_fn_calls;
pub(super) use import_resolution::has_name_clash_parts;
pub(super) use module_idx::ModuleIdx;
pub(super) use module_idx::module_idx;
pub(super) use path_resolution::associated_receiver_parts;
pub(super) use path_resolution::is_import_style_path;
pub(super) use path_resolution::path_parts;

mod fn_call_analysis;
mod fn_path_resolution;
mod import_resolution;
mod module_idx;
mod path_resolution;
mod scope_bindings;
