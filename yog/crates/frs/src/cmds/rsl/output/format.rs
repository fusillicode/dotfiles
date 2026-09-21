use super::ViolationOutputFormat;
use crate::cmds::rsl::rules::common::Location;

mod aliased_import;
mod misordered_fn;
mod misordered_item_group;
mod misordered_visibility;
mod nonadjacent_impl;
mod overqualified_call;
mod qualified_item;
mod relative_path;
mod unqualified_call;

pub trait FormattedRuleViolation: Send + Sync + 'static {
    fn format(&self, output_format: ViolationOutputFormat) -> String;
}

pub(super) fn format_violation(
    location: &Location,
    code: &str,
    suggested_fix: &str,
    output_format: ViolationOutputFormat,
) -> String {
    let location = format_location(location);
    match output_format {
        ViolationOutputFormat::Compact => format!("{location},{suggested_fix}"),
        ViolationOutputFormat::Debug => format!("{location},{code},{suggested_fix}"),
    }
}

fn format_location(location: &Location) -> String {
    format!("{}:{}:{}", location.file.display(), location.line, location.column)
}
