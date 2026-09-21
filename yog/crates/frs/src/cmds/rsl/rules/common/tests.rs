use std::path::PathBuf;

use test_that::prelude::*;

use super::super::aliased_import::AliasedImportRule;
use super::super::aliased_import::AliasedImportViolation;
use super::super::common::CallDetails;
use super::super::overqualified_call::OverqualifiedCallRule;
use super::super::overqualified_call::OverqualifiedCallViolation;
use super::super::qualified_item::QUALIFIED_ALLOWED_PATHS;
use super::super::qualified_item::QualifiedItemDetails;
use super::super::qualified_item::QualifiedItemRule;
use super::super::qualified_item::QualifiedItemViolation;
use super::super::unqualified_call::UnqualifiedCallRule;
use super::super::unqualified_call::UnqualifiedCallViolation;
use crate::cmds::rsl::rules::TypedRule;

fn qualified_item_rule() -> QualifiedItemRule {
    QualifiedItemRule::new(&[])
}

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
fn test_qualified_item_check_when_non_fn_path_is_fully_qualified_reports_import() {
    let syntax = syn::parse_file(
        r"
            mod values {
                pub const VALUE: usize = 1;
            }
            fn read() -> usize {
                crate::values::VALUE
            }
            ",
    )
    .unwrap();

    let result = qualified_item_rule().check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(
        result,
        eq(vec![QualifiedItemViolation {
            file: PathBuf::from("test.rs"),
            line: 6,
            column: 17,
            details: QualifiedItemDetails {
                actual_path: "crate::values::VALUE".to_owned(),
                expected_import: "use crate::values::VALUE;".to_owned(),
            },
        }])
    );
}

#[test]
fn test_qualified_item_check_when_path_is_allowed_returns_no_violations() {
    let syntax = syn::parse_file(
        r"
            fn inspect(_: std::fmt::Result) -> rootcause::Result<()> {
                panic!()
            }
            ",
    )
    .unwrap();

    let result = QualifiedItemRule::new(QUALIFIED_ALLOWED_PATHS).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(result, is_empty());
}

#[test]
fn test_qualified_item_check_when_path_is_not_allowed_reports_import() {
    let syntax = syn::parse_file(
        r"
            fn inspect(_: std::fmt::Formatter<'_>) {}
            ",
    )
    .unwrap();

    let result = QualifiedItemRule::new(QUALIFIED_ALLOWED_PATHS).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(
        result,
        eq(vec![QualifiedItemViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 27,
            details: QualifiedItemDetails {
                actual_path: "std::fmt::Formatter".to_owned(),
                expected_import: "use std::fmt::Formatter;".to_owned(),
            },
        }])
    );
}

#[test]
fn test_qualified_item_check_when_non_fn_name_clashes_allows_qualified_path() {
    let syntax = syn::parse_file(
        r"
            mod values {
                pub const VALUE: usize = 1;
            }
            const VALUE: usize = 2;
            fn read() -> usize {
                crate::values::VALUE
            }
            ",
    )
    .unwrap();

    let result = qualified_item_rule().check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(result, is_empty());
}

#[test]
fn test_qualified_item_check_when_struct_names_clash_allows_one_qualified_path() {
    let syntax = syn::parse_file(
        r"
            mod first {
                pub struct Thing;
            }
            mod second {
                pub struct Thing;
            }
            use crate::first::Thing;
            fn read() -> crate::second::Thing {
                panic!()
            }
            ",
    )
    .unwrap();

    let result = qualified_item_rule().check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(result, is_empty());
}

#[test]
fn test_aliased_import_check_when_alias_is_private_reports_alias() {
    let syntax = syn::parse_file(
        r"
            use std::fmt::Display as Formatter;
            ",
    )
    .unwrap();

    let result = AliasedImportRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(
        result,
        eq(vec![AliasedImportViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 38,
            unaliased_import: "std::fmt::Display".to_owned(),
        }])
    );
}

#[test]
fn test_aliased_import_check_when_alias_is_wildcard_returns_no_violations() {
    let syntax = syn::parse_file(
        r"
            use std::fmt::Display as _;
            ",
    )
    .unwrap();

    let result = AliasedImportRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(result, is_empty());
}

#[test]
fn test_aliased_import_check_when_public_reexport_is_renamed_reports_alias() {
    let syntax = syn::parse_file(
        r"
            pub use std::fmt::Debug as Formatter;
            ",
    )
    .unwrap();

    let result = AliasedImportRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(
        result,
        eq(vec![AliasedImportViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 40,
            unaliased_import: "std::fmt::Debug".to_owned(),
        }])
    );
}

#[test]
fn test_aliased_import_check_when_reexport_alias_is_restricted_reports_alias() {
    let syntax = syn::parse_file(
        r"
            pub(crate) use std::fmt::Debug as Formatter;
            ",
    )
    .unwrap();

    let result = AliasedImportRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(
        result,
        eq(vec![AliasedImportViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 47,
            unaliased_import: "std::fmt::Debug".to_owned(),
        }])
    );
}

#[test]
fn test_qualified_item_check_when_external_paths_are_qualified_reports_paths() {
    let syntax = syn::parse_file(
        r"
            mod external;
            use external::Thing;
            struct Data;
            impl Data {
                fn run() {}
                fn call(&self) {
                    self.run();
                    Self::run();
                }
            }
            fn read() -> external::Thing {
                external::Thing::new()
            }
            ",
    )
    .unwrap();

    let result = qualified_item_rule().check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(
        result,
        eq(vec![
            QualifiedItemViolation {
                file: PathBuf::from("test.rs"),
                line: 12,
                column: 26,
                details: QualifiedItemDetails {
                    actual_path: "external::Thing".to_owned(),
                    expected_import: "use external::Thing;".to_owned(),
                },
            },
            QualifiedItemViolation {
                file: PathBuf::from("test.rs"),
                line: 13,
                column: 17,
                details: QualifiedItemDetails {
                    actual_path: "external::Thing".to_owned(),
                    expected_import: "use external::Thing;".to_owned(),
                },
            },
        ])
    );
}

#[test]
fn test_qualified_item_check_when_unknown_external_type_is_qualified_reports_import() {
    let syntax = syn::parse_file(
        r"
            fn inspect(_: syn::ExprCall) {}
            ",
    )
    .unwrap();

    let result = qualified_item_rule().check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(
        result,
        eq(vec![QualifiedItemViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 27,
            details: QualifiedItemDetails {
                actual_path: "syn::ExprCall".to_owned(),
                expected_import: "use syn::ExprCall;".to_owned(),
            },
        }])
    );
}

#[test]
fn test_qualified_item_check_when_enum_variant_is_qualified_ignores_path() {
    let syntax = syn::parse_file(
        r"
            enum Kind {
                First,
                Second,
            }
            fn select(kind: Kind) -> Kind {
                match kind {
                    Kind::First => Kind::Second,
                    Kind::Second => Kind::First,
                }
            }
            ",
    )
    .unwrap();

    let result = qualified_item_rule().check(&crate::cmds::rsl::rules::test_ctx(&syntax));

    assert_that!(result, is_empty());
}

#[test]
fn test_qualified_item_check_when_associated_fn_is_referenced_ignores_path() {
    let syntax = syn::parse_file(
        r"
            fn converter() -> fn(&String) -> &str {
                String::as_str
            }
            ",
    )
    .unwrap();

    let result = qualified_item_rule().check(&crate::cmds::rsl::rules::test_ctx(&syntax));

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

#[test]
fn test_qualified_item_violation_formats_compact_output() {
    let violation = QualifiedItemViolation {
        file: PathBuf::from("test.rs"),
        line: 3,
        column: 13,
        details: QualifiedItemDetails {
            actual_path: "external::Thing".to_owned(),
            expected_import: "use external::Thing;".to_owned(),
        },
    };

    assert_eq!(
        violation.to_string(),
        "test.rs:3:13,qualified_item,replace `external::Thing` with `Thing`; add `use external::Thing;`"
    );
}

#[test]
fn test_aliased_import_violation_formats_compact_output() {
    let violation = AliasedImportViolation {
        file: PathBuf::from("test.rs"),
        line: 4,
        column: 13,
        unaliased_import: "std::fmt::Thing".to_owned(),
    };

    assert_eq!(
        violation.to_string(),
        "test.rs:4:13,aliased_import,use unaliased import `std::fmt::Thing` if it doesn't clash"
    );
}
