use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::qualified_item::QualifiedItemRule;
use crate::cmds::rsl::rules::qualified_item::QualifiedItemViolation;

impl FormattedRuleViolation for QualifiedItemViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        let imported_name = self
            .details
            .actual_path
            .rsplit("::")
            .next()
            .unwrap_or(self.details.actual_path.as_str());
        format_violation(
            &self.location,
            <QualifiedItemRule as TypedRule>::code(),
            &format!(
                "replace `{}` with `{}`; add `{}`",
                self.details.actual_path, imported_name, self.details.expected_import
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
    use crate::cmds::rsl::rules::qualified_item::QualifiedItemDetails;
    use crate::cmds::rsl::rules::qualified_item::QualifiedItemViolation;

    #[test]
    fn test_qualified_item_violation_when_details_are_present_formats_compact_output() {
        let violation = QualifiedItemViolation {
            location: Location::new(PathBuf::from("test.rs"), 3, 13),
            details: QualifiedItemDetails {
                actual_path: "external::Thing".to_owned(),
                expected_import: "use external::Thing;".to_owned(),
            },
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:3:13,replace `external::Thing` with `Thing`; add `use external::Thing;`"
        );
    }
}
