//! Qualified-item rule for `frs rsl`.

use std::path::Path;

use proc_macro2::Span;
use syn::Expr;
use syn::spanned::Spanned;
use syn::visit::Visit;

use super::common::Location;
use super::common::ModuleIdx;
use super::common::associated_receiver_parts;
use super::common::has_name_clash_parts;
use super::common::is_import_style_path;
use super::common::module_idx;
use super::common::path_parts;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

pub(super) const QUALIFIED_ALLOWED_PATHS: &[&str] = &[
    "anyhow::Result",
    "rootcause::Result",
    "std::fmt::Result",
    "std::io::Result",
];

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct QualifiedItemViolation {
    pub location: Location,
    pub details: QualifiedItemDetails,
}

impl QualifiedItemViolation {
    pub(super) fn new(path: &Path, span: Span, actual_path: String, expected_import: String) -> Self {
        Self {
            location: Location::from_span(path, span),
            details: QualifiedItemDetails {
                actual_path,
                expected_import,
            },
        }
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct QualifiedItemDetails {
    pub actual_path: String,
    pub expected_import: String,
}

pub struct QualifiedItemRule {
    allowed_paths: &'static [&'static str],
}

impl QualifiedItemRule {
    pub(super) const fn new(allowed_paths: &'static [&'static str]) -> Self {
        Self { allowed_paths }
    }
}

impl TypedRule for QualifiedItemRule {
    type Violation = QualifiedItemViolation;

    fn code() -> &'static str {
        "qualified_item"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let idx = module_idx(ctx.file);
        let mut violations = Vec::new();

        for scope in &idx.scopes {
            let mut visitor = QualifiedItemVisitor {
                idx: &idx,
                current_module: &scope.path,
                source_path: ctx.path,
                allowed_paths: self.allowed_paths,
                violations: &mut violations,
                skip_call_path: false,
            };
            for item in scope.items {
                visitor.visit_item(item);
            }
        }

        violations
    }
}

struct QualifiedItemVisitor<'idx, 'ast, 'output> {
    idx: &'idx ModuleIdx<'ast>,
    current_module: &'idx [String],
    source_path: &'idx Path,
    allowed_paths: &'static [&'static str],
    violations: &'output mut Vec<QualifiedItemViolation>,
    skip_call_path: bool,
}

impl<'ast> Visit<'ast> for QualifiedItemVisitor<'_, '_, '_> {
    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && let Some(parts) = associated_receiver_parts(&path.path)
        {
            check_non_fn_path(self, &parts, path.path.span());
        }

        let previous = self.skip_call_path;
        self.skip_call_path = matches!(expression.func.as_ref(), Expr::Path(_));
        syn::visit::visit_expr_call(self, expression);
        self.skip_call_path = previous;
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if self.skip_call_path {
            self.skip_call_path = false;
        } else if let Some(parts) = path_parts(path) {
            check_non_fn_path(self, &parts, path.span());
        }

        syn::visit::visit_path(self, path);
    }

    fn visit_item_mod(&mut self, _module: &'ast syn::ItemMod) {}

    fn visit_item_use(&mut self, _item_use: &'ast syn::ItemUse) {}

    fn visit_attribute(&mut self, _attribute: &'ast syn::Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast syn::Macro) {}
}

fn check_non_fn_path(visitor: &mut QualifiedItemVisitor<'_, '_, '_>, parts: &[String], span: Span) {
    if parts.len() <= 1 || !is_import_style_path(parts) {
        return;
    }

    let actual_path = parts.join("::");
    if visitor.allowed_paths.iter().any(|allowed| *allowed == actual_path)
        || has_name_clash_parts(visitor.idx, visitor.current_module, parts)
    {
        return;
    }

    visitor.violations.push(QualifiedItemViolation::new(
        visitor.source_path,
        span,
        actual_path.clone(),
        format!("use {actual_path};"),
    ));
}

impl TypedRuleViolation for QualifiedItemViolation {
    type Rule = QualifiedItemRule;
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::*;
    use crate::cmds::rsl::rules::TypedRule;
    use crate::cmds::rsl::rules::common::Location;

    fn qualified_item_rule() -> QualifiedItemRule {
        QualifiedItemRule::new(&[])
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
                location: Location::new(PathBuf::from("test.rs"), 6, 17),
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
                location: Location::new(PathBuf::from("test.rs"), 2, 27),
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
                    location: Location::new(PathBuf::from("test.rs"), 12, 26),
                    details: QualifiedItemDetails {
                        actual_path: "external::Thing".to_owned(),
                        expected_import: "use external::Thing;".to_owned(),
                    },
                },
                QualifiedItemViolation {
                    location: Location::new(PathBuf::from("test.rs"), 13, 17),
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
                location: Location::new(PathBuf::from("test.rs"), 2, 27),
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
}
