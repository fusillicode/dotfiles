//! Impl-adjacency rule.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use serde::Serialize;

use crate::rsl::ast::ItemKind;
use crate::rsl::engine::FileContext;
use crate::rsl::rules::TypedRule;
use crate::rsl::rules::TypedRuleViolation;

pub struct ImplAdjacencyRule;

impl TypedRule for ImplAdjacencyRule {
    type Violation = ImplAdjacencyViolation;

    fn name() -> &'static str {
        "impl_adjacency"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let mut violations = Vec::new();

        for items in crate::rsl::ast::module_scopes(ctx.file) {
            // Raw AST positions make opaque macros and cfg-decorated items break physical adjacency.
            for node in &crate::rsl::ast::module_nodes(items) {
                if node.indices.len() < 2 {
                    continue;
                }

                for pair in node.indices.windows(2) {
                    let [previous, current] = pair else {
                        continue;
                    };
                    if *current == previous.saturating_add(1) {
                        continue;
                    }

                    let Some(current_item) = items.get(*current) else {
                        continue;
                    };
                    let Some(previous_item) = items.get(*previous) else {
                        continue;
                    };
                    let Some(classified) = crate::rsl::ast::classify_item(current_item) else {
                        continue;
                    };
                    violations.push(ImplAdjacencyViolation::new(
                        ctx.path,
                        classified.span,
                        crate::rsl::ast::impl_order_label(previous_item),
                        classified.kind,
                    ));
                }
            }
        }

        violations
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Serialize)]
pub(super) struct ImplAdjacencyViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    message: &'static str,
    details: ImplAdjacencyDetails,
}

impl ImplAdjacencyViolation {
    fn new(path: &Path, span: Span, expected_after: String, item: ItemKind) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "impl must follow its type",
            details: ImplAdjacencyDetails { expected_after, item },
        }
    }
}

impl TypedRuleViolation for ImplAdjacencyViolation {
    type Rule = ImplAdjacencyRule;
}

impl Display for ImplAdjacencyViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            self.message,
            &format!("{} -> after {}", self.details.item.label(), self.details.expected_after),
        ))
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Serialize)]
struct ImplAdjacencyDetails {
    expected_after: String,
    item: ItemKind,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::ImplAdjacencyDetails;
    use super::ImplAdjacencyRule;
    use super::ImplAdjacencyViolation;
    use crate::rsl::ast::ItemKind;
    use crate::rsl::rules::TypedRule;

    #[test]
    fn test_impl_adjacency_rule_check_when_impl_is_not_adjacent_to_type_reports_impl() {
        let syntax = syn::parse_file(
            r"
            struct Data;
            opaque!();
            impl Data {}
            ",
        )
        .unwrap();

        let result = ImplAdjacencyRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![ImplAdjacencyViolation {
                file: PathBuf::from("test.rs"),
                line: 4,
                column: 13,
                message: "impl must follow its type",
                details: ImplAdjacencyDetails {
                    expected_after: "struct Data".to_owned(),
                    item: ItemKind::Impl,
                },
            }])
        );
    }

    #[test]
    fn test_impl_adjacency_rule_check_when_trait_impl_precedes_inherent_impl_reports_impls() {
        let syntax = syn::parse_file(
            r"
            struct Data;
            impl Behavior for Data {}
            impl Data {}
            ",
        )
        .unwrap();

        let result = ImplAdjacencyRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![
                ImplAdjacencyViolation {
                    file: PathBuf::from("test.rs"),
                    line: 4,
                    column: 13,
                    message: "impl must follow its type",
                    details: ImplAdjacencyDetails {
                        expected_after: "struct Data".to_owned(),
                        item: ItemKind::Impl,
                    },
                },
                ImplAdjacencyViolation {
                    file: PathBuf::from("test.rs"),
                    line: 3,
                    column: 13,
                    message: "impl must follow its type",
                    details: ImplAdjacencyDetails {
                        expected_after: "inherent impl Data".to_owned(),
                        item: ItemKind::Impl,
                    },
                },
            ])
        );
    }

    #[test]
    fn test_impl_adjacency_violation_when_details_are_present_formats_compact_output() {
        let violation = ImplAdjacencyViolation {
            file: PathBuf::from("test.rs"),
            line: 4,
            column: 13,
            message: "impl must follow its type",
            details: ImplAdjacencyDetails {
                expected_after: "struct Data".to_owned(),
                item: ItemKind::Impl,
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:4:13 impl must follow its type - impl -> after struct Data"
        );
    }

    #[test]
    fn test_impl_adjacency_rule_check_when_cfg_item_is_between_type_and_impl_reports_impl() {
        let syntax = syn::parse_file(
            r#"
            struct Data;
            #[cfg(feature = "extra")]
            fn extra() {}
            impl Data {}
            "#,
        )
        .unwrap();

        let result = ImplAdjacencyRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![ImplAdjacencyViolation {
                file: PathBuf::from("test.rs"),
                line: 5,
                column: 13,
                message: "impl must follow its type",
                details: ImplAdjacencyDetails {
                    expected_after: "struct Data".to_owned(),
                    item: ItemKind::Impl,
                },
            }])
        );
    }
}
