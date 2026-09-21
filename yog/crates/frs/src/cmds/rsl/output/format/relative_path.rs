use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::relative_path::RelativePathRule;
use crate::cmds::rsl::rules::relative_path::RelativePathViolation;

impl FormattedRuleViolation for RelativePathViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        format_violation(
            &self.location,
            <RelativePathRule as TypedRule>::code(),
            "use a crate-absolute path",
            output_format,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::super::ViolationOutputFormat;
    use super::super::FormattedRuleViolation;
    use crate::cmds::rsl::rules::common::Location;
    use crate::cmds::rsl::rules::relative_path::RelativePathViolation;

    #[test]
    fn test_relative_path_violation_when_details_are_present_formats_compact_output() {
        let violation = RelativePathViolation {
            location: Location::new(PathBuf::from("test.rs"), 3, 13),
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:3:13,use a crate-absolute path"
        );
    }

    #[test]
    fn test_formatted_rule_violation_when_debug_format_is_selected_includes_code() {
        let violation = RelativePathViolation {
            location: Location::new(PathBuf::from("test.rs"), 3, 13),
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Debug),
            "test.rs:3:13,relative_path,use a crate-absolute path"
        );
    }
}
