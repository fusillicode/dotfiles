//! Shared components for frs rsl rules.

use std::path::Path;
use std::path::PathBuf;

pub use fn_call_analysis::CallDetails;
pub(super) use fn_call_analysis::FnCallFinding;
pub(super) use fn_call_analysis::FnCallKind;
pub(super) use fn_call_analysis::find_fn_calls;
pub(super) use import_resolution::has_name_clash_parts;
pub(super) use module_idx::ModuleIdx;
pub(super) use module_idx::module_idx;
pub(super) use path_resolution::associated_receiver_parts;
pub(super) use path_resolution::is_import_style_path;
pub(super) use path_resolution::path_parts;
use proc_macro2::Span;

mod fn_call_analysis;
mod fn_path_resolution;
mod import_resolution;
mod module_idx;
mod path_resolution;
mod scope_bindings;

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct Location {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
}

impl Location {
    pub const fn new(file: PathBuf, line: usize, column: usize) -> Self {
        Self { file, line, column }
    }

    pub fn from_span(path: &Path, span: Span) -> Self {
        let start = span.start();
        Self::new(path.to_path_buf(), start.line, start.column.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use syn::spanned::Spanned;

    use super::Location;

    #[test]
    fn test_location_from_span_when_span_starts_at_file_start_uses_one_based_column() {
        let item: syn::ItemFn = syn::parse_str("fn main() {}").unwrap();

        let actual = Location::from_span(Path::new("test.rs"), item.span());

        assert_eq!(
            actual,
            Location {
                file: PathBuf::from("test.rs"),
                line: 1,
                column: 1,
            }
        );
    }
}
