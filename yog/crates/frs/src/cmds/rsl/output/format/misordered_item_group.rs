use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::misordered_item_group::MisorderedItemGroupRule;
use crate::cmds::rsl::rules::misordered_item_group::MisorderedItemGroupViolation;

impl FormattedRuleViolation for MisorderedItemGroupViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        format_violation(
            &self.location,
            <MisorderedItemGroupRule as TypedRule>::code(),
            &format!(
                "move `{}` after `{}`",
                self.details.item.label(),
                self.details.expected_group
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
    use crate::cmds::rsl::ast::ItemGroup;
    use crate::cmds::rsl::ast::ItemKind;
    use crate::cmds::rsl::rules::common::Location;
    use crate::cmds::rsl::rules::misordered_item_group::MisorderedItemGroupDetails;
    use crate::cmds::rsl::rules::misordered_item_group::MisorderedItemGroupViolation;

    #[test]
    fn test_misordered_item_group_violation_when_details_are_present_formats_compact_output() {
        let violation = MisorderedItemGroupViolation {
            location: Location::new(PathBuf::from("test.rs"), 4, 13),
            details: MisorderedItemGroupDetails {
                expected_group: ItemGroup::Items,
                item: ItemKind::Const,
            },
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:4:13,move `const` after `items`"
        );
    }
}
