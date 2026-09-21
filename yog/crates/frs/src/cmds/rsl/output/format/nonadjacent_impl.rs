use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::nonadjacent_impl::NonadjacentImplRule;
use crate::cmds::rsl::rules::nonadjacent_impl::NonadjacentImplViolation;

impl FormattedRuleViolation for NonadjacentImplViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        format_violation(
            &self.location,
            <NonadjacentImplRule as TypedRule>::code(),
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
    use crate::cmds::rsl::rules::nonadjacent_impl::NonadjacentImplDetails;
    use crate::cmds::rsl::rules::nonadjacent_impl::NonadjacentImplViolation;

    #[test]
    fn test_nonadjacent_impl_violation_when_details_are_present_formats_compact_output() {
        let violation = NonadjacentImplViolation {
            location: Location::new(PathBuf::from("test.rs"), 4, 13),
            details: NonadjacentImplDetails {
                expected_after: "struct Data".to_owned(),
                item: ItemKind::Impl,
            },
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:4:13,move `impl` after `struct Data`"
        );
    }
}
