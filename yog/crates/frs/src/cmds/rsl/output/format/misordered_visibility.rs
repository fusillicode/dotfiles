use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::misordered_visibility::MisorderedVisibilityRule;
use crate::cmds::rsl::rules::misordered_visibility::MisorderedVisibilityViolation;

impl FormattedRuleViolation for MisorderedVisibilityViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        format_violation(
            &self.location,
            <MisorderedVisibilityRule as TypedRule>::code(),
            &format!(
                "move `{}` before `{}`",
                self.details.item_label, self.details.expected_before
            ),
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
    use crate::cmds::rsl::rules::misordered_visibility::MisorderedVisibilityDetails;
    use crate::cmds::rsl::rules::misordered_visibility::MisorderedVisibilityViolation;

    #[test]
    fn test_misordered_visibility_violation_when_details_are_present_formats_compact_output() {
        let violation = MisorderedVisibilityViolation {
            location: Location::new(PathBuf::from("test.rs"), 3, 17),
            details: MisorderedVisibilityDetails {
                expected_before: "fn private".to_owned(),
                item_label: "fn public".to_owned(),
            },
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:3:17,move `fn public` before `fn private`"
        );
    }
}
