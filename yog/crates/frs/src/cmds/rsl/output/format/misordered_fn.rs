use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::misordered_fn::MisorderedFnRule;
use crate::cmds::rsl::rules::misordered_fn::MisorderedFnViolation;

impl FormattedRuleViolation for MisorderedFnViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        format_violation(
            &self.location,
            <MisorderedFnRule as TypedRule>::code(),
            &format!(
                "move `{}` after `{}`",
                self.details.item.label(),
                self.details.expected_after
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
    use crate::cmds::rsl::ast::ItemKind;
    use crate::cmds::rsl::rules::common::Location;
    use crate::cmds::rsl::rules::misordered_fn::MisorderedFnDetails;
    use crate::cmds::rsl::rules::misordered_fn::MisorderedFnViolation;

    #[test]
    fn test_misordered_fn_violation_when_details_are_present_formats_compact_output() {
        let violation = MisorderedFnViolation {
            location: Location::new(PathBuf::from("test.rs"), 2, 13),
            details: MisorderedFnDetails {
                expected_after: "fn caller".to_owned(),
                item: ItemKind::Fn,
            },
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:2:13,move `fn` after `fn caller`"
        );
    }
}
