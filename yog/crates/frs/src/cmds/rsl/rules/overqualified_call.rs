//! Overqualified fn-call rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use super::common::CallDetails;
use super::common::FnCallFinding;
use super::common::FnCallKind;
use super::common::find_fn_calls;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

pub struct OverqualifiedCallRule;

impl TypedRule for OverqualifiedCallRule {
    type Violation = OverqualifiedCallViolation;

    fn code() -> &'static str {
        "oc"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        find_fn_calls(ctx.file, FnCallKind::Overqualified)
            .into_iter()
            .map(|finding| OverqualifiedCallViolation::new(ctx.path, finding))
            .collect()
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) struct OverqualifiedCallViolation {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) details: CallDetails,
}

impl OverqualifiedCallViolation {
    fn new(path: &Path, finding: FnCallFinding) -> Self {
        let location = finding.span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            details: CallDetails {
                actual_path: finding.actual_path,
                replacement_path: finding.suggestion.expected_path,
                add_import: finding.suggestion.required_import,
            },
        }
    }
}

impl TypedRuleViolation for OverqualifiedCallViolation {
    type Rule = OverqualifiedCallRule;
}

impl Display for OverqualifiedCallViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        let details = format!(
            "replace `{}` with `{}`{}",
            self.details.actual_path,
            self.details.replacement_path,
            self.details
                .add_import
                .as_ref()
                .map_or_else(String::new, |import| format!("; add `{import}`")),
        );
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            OverqualifiedCallRule::code(),
            &details,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::*;
    use crate::cmds::rsl::rules::TypedRule;

    #[test]
    fn test_overqualified_call_check_when_foreign_module_call_has_one_module_prefix_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod helper {
                pub fn run() {}
            }
            fn main() {
                helper::run();
            }
            ",
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_foreign_module_call_uses_crate_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod helper {
                pub fn run() {}
            }
            fn main() {
                crate::helper::run();
            }
            ",
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_free_fn_call_has_one_module_prefix_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            fn read() {
                fs::read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_free_fn_call_has_multiple_module_prefixes_reports_call() {
        let syntax = syn::parse_file(
            r#"
            fn read() {
                std::fs::read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![OverqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 3,
                column: 17,
                details: CallDetails {
                    actual_path: "std::fs::read_to_string".to_owned(),
                    replacement_path: "fs::read_to_string".to_owned(),
                    add_import: Some("use std::fs;".to_owned()),
                },
            }])
        );
    }

    #[test]
    fn test_overqualified_call_check_when_shortened_module_name_conflicts_with_import_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            mod other {
                pub mod fs {}
            }
            use crate::other::fs;
            fn read() {
                std::fs::read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_shortened_module_name_conflicts_with_local_module_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            mod fs {}
            fn read() {
                std::fs::read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_imported_fn_module_name_conflicts_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod source {
                pub fn run() {}
            }
            mod other {
                pub mod source {}
            }
            use crate::other::source;
            use crate::source::run;
            fn main() {
                run();
            }
            ",
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_violation_formats_compact_output() {
        let violation = OverqualifiedCallViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 13,
            details: CallDetails {
                actual_path: "run()".to_owned(),
                replacement_path: "external::run()".to_owned(),
                add_import: Some("use crate::external;".to_owned()),
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:2:13,oc,replace `run()` with `external::run()`; add `use crate::external;`"
        );
    }
}
