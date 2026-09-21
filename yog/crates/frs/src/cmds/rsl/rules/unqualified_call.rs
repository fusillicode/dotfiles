//! Unqualified fn-call rule for `frs rsl`.

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

pub struct UnqualifiedCallRule;

impl TypedRule for UnqualifiedCallRule {
    type Violation = UnqualifiedCallViolation;

    fn code() -> &'static str {
        "uc"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        find_fn_calls(ctx.file, FnCallKind::Unqualified)
            .into_iter()
            .map(|finding| UnqualifiedCallViolation::new(ctx.path, finding))
            .collect()
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) struct UnqualifiedCallViolation {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) details: CallDetails,
}

impl UnqualifiedCallViolation {
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

impl TypedRuleViolation for UnqualifiedCallViolation {
    type Rule = UnqualifiedCallRule;
}

impl Display for UnqualifiedCallViolation {
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
            UnqualifiedCallRule::code(),
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
    fn test_unqualified_call_check_when_same_module_call_is_bare_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}
            fn run() {
                helper();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_same_module_call_is_bare_with_glob_import_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod tests {
                use super::*;

                fn helper() {}

                fn run() {
                    helper();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_parent_fn_is_bare_with_super_glob_import_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}

            mod tests {
                use super::*;
                use test_that::prelude::*;

                fn invoke() {
                    helper();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_explicit_import_is_mixed_with_glob_import_reports_imported_module() {
        let syntax = syn::parse_file(
            r"
            mod external {
                pub fn run() {}
            }
            mod tests {
                use super::*;
                use crate::external::run;

                fn invoke() {
                    run();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![UnqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 10,
                column: 21,
                details: CallDetails {
                    actual_path: "run".to_owned(),
                    replacement_path: "external::run".to_owned(),
                    add_import: Some("use crate::external;".to_owned()),
                },
            }])
        );
    }

    #[test]
    fn test_unqualified_call_check_when_parent_explicit_import_is_reexported_by_glob_reports_imported_module() {
        let syntax = syn::parse_file(
            r"
            mod external {
                pub fn run() {}
            }
            use crate::external::run;

            mod tests {
                use super::*;

                fn invoke() {
                    run();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![UnqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 11,
                column: 21,
                details: CallDetails {
                    actual_path: "run".to_owned(),
                    replacement_path: "external::run".to_owned(),
                    add_import: Some("use crate::external;".to_owned()),
                },
            }])
        );
    }

    #[test]
    fn test_unqualified_call_check_when_glob_imported_call_is_unresolved_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod external {
                pub fn imported() {}
            }
            mod tests {
                use super::*;

                fn run() {
                    imported();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_same_module_call_uses_self_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}
            fn run() {
                self::helper();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_nested_fn_call_is_local_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn run() {
                fn helper() {}
                helper();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_callable_parameter_is_called_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn run(check: impl Fn()) {
                check();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_closure_binding_is_called_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn run() {
                let check = || {};
                check();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_local_binding_shadows_imported_fn_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod external {
                pub fn check() {}
            }
            use external::check;
            fn run() {
                let check = || {};
                check();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_imported_foreign_call_is_bare_reports_call() {
        let syntax = syn::parse_file(
            r"
            mod external;
            use external::run;
            fn main() {
                run();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![UnqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 5,
                column: 17,
                details: CallDetails {
                    actual_path: "run".to_owned(),
                    replacement_path: "external::run".to_owned(),
                    add_import: None,
                },
            }])
        );
    }

    #[test]
    fn test_unqualified_call_check_when_imported_external_crate_call_is_bare_omits_import() {
        let syntax = syn::parse_file(
            r"
            use tempfile::tempdir;
            fn main() {
                tempdir();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![UnqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 4,
                column: 17,
                details: CallDetails {
                    actual_path: "tempdir".to_owned(),
                    replacement_path: "tempfile::tempdir".to_owned(),
                    add_import: None,
                },
            }])
        );
    }

    #[test]
    fn test_unqualified_call_check_when_associated_fn_receiver_is_uppercase_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            fn open() {
                File::open("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_bare_uppercase_constructor_is_called_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn read() {
                Ok(());
                Err(());
                Some(1);
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_bare_prelude_fn_is_called_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn read(value: usize) {
                drop(value);
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_unknown_bare_fn_call_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            fn read() {
                read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_nested_module_call_is_bare_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod outer {
                fn helper() {}
                fn run() {
                    helper();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_violation_formats_compact_output() {
        let violation = UnqualifiedCallViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 13,
            details: CallDetails {
                actual_path: "run()".to_owned(),
                replacement_path: "self::run()".to_owned(),
                add_import: None,
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:2:13,uc,replace `run()` with `self::run()`"
        );
    }
}
