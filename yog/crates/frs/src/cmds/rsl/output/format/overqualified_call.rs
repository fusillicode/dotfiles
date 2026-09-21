use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::overqualified_call::OverqualifiedCallRule;
use crate::cmds::rsl::rules::overqualified_call::OverqualifiedCallViolation;

impl FormattedRuleViolation for OverqualifiedCallViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        let details = format!(
            "replace `{}` with `{}`{}",
            self.details.actual_path,
            self.details.replacement_path,
            self.details
                .add_import
                .as_ref()
                .map_or_else(String::new, |import| format!("; add `{import}`")),
        );
        format_violation(
            &self.location,
            <OverqualifiedCallRule as TypedRule>::code(),
            &details,
            output_format,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::super::ViolationOutputFormat;
    use super::super::FormattedRuleViolation;
    use crate::cmds::rsl::rules::common::CallDetails;
    use crate::cmds::rsl::rules::common::Location;
    use crate::cmds::rsl::rules::overqualified_call::OverqualifiedCallViolation;

    #[test]
    fn test_overqualified_call_violation_when_details_are_present_formats_compact_output() {
        let violation = OverqualifiedCallViolation {
            location: Location::new(PathBuf::from("test.rs"), 2, 13),
            details: CallDetails {
                actual_path: "run()".to_owned(),
                replacement_path: "external::run()".to_owned(),
                add_import: Some("use crate::external;".to_owned()),
            },
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:2:13,replace `run()` with `external::run()`; add `use crate::external;`"
        );
    }
}
