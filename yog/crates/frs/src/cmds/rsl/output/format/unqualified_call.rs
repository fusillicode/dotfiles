use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::unqualified_call::UnqualifiedCallRule;
use crate::cmds::rsl::rules::unqualified_call::UnqualifiedCallViolation;

impl FormattedRuleViolation for UnqualifiedCallViolation {
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
            <UnqualifiedCallRule as TypedRule>::code(),
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
    use crate::cmds::rsl::rules::unqualified_call::UnqualifiedCallViolation;

    #[test]
    fn test_unqualified_call_violation_when_details_are_present_formats_compact_output() {
        let violation = UnqualifiedCallViolation {
            location: Location::new(PathBuf::from("test.rs"), 2, 13),
            details: CallDetails {
                actual_path: "run()".to_owned(),
                replacement_path: "self::run()".to_owned(),
                add_import: None,
            },
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:2:13,replace `run()` with `self::run()`"
        );
    }
}
