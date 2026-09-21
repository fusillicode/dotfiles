use super::super::ViolationOutputFormat;
use super::FormattedRuleViolation;
use super::format_violation;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::aliased_import::AliasedImportRule;
use crate::cmds::rsl::rules::aliased_import::AliasedImportViolation;

impl FormattedRuleViolation for AliasedImportViolation {
    fn format(&self, output_format: ViolationOutputFormat) -> String {
        format_violation(
            &self.location,
            <AliasedImportRule as TypedRule>::code(),
            &format!("use unaliased import `{}` if it doesn't clash", self.unaliased_import),
            output_format,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::super::ViolationOutputFormat;
    use super::super::FormattedRuleViolation;
    use crate::cmds::rsl::rules::aliased_import::AliasedImportViolation;
    use crate::cmds::rsl::rules::common::Location;

    #[test]
    fn test_aliased_import_violation_when_details_are_present_formats_compact_output() {
        let violation = AliasedImportViolation {
            location: Location::new(PathBuf::from("test.rs"), 4, 13),
            unaliased_import: "std::fmt::Thing".to_owned(),
        };

        assert_eq!(
            FormattedRuleViolation::format(&violation, ViolationOutputFormat::Compact),
            "test.rs:4:13,use unaliased import `std::fmt::Thing` if it doesn't clash"
        );
    }
}
