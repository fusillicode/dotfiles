//! Visibility-order rule.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use serde::Serialize;
use syn::Item;

use crate::rsl::ast::ItemKind;
use crate::rsl::ast::VisibilityClass;
use crate::rsl::engine::FileContext;
use crate::rsl::rules::TypedRule;
use crate::rsl::rules::TypedRuleViolation;

pub struct VisibilityOrderRule {
    visibility_order: [VisibilityClass; 4],
}

impl VisibilityOrderRule {
    pub(super) fn new(visibility_order: Option<[VisibilityClass; 4]>) -> Self {
        Self {
            visibility_order: visibility_order.unwrap_or([
                VisibilityClass::Public,
                VisibilityClass::Crate,
                VisibilityClass::Restricted,
                VisibilityClass::Private,
            ]),
        }
    }

    fn check_visibility_order(
        &self,
        nodes: &[crate::rsl::ast::OrderNode],
        path: &Path,
        violations: &mut Vec<VisibilityOrderViolation>,
    ) {
        let mut current_group = None;
        let mut highest_visibility: Option<&crate::rsl::ast::OrderNode> = None;

        for node in nodes {
            if node.group != current_group {
                current_group = node.group;
                highest_visibility = None;
            }

            let Some(visibility) = node.visibility else {
                continue;
            };

            if let Some(previous) = highest_visibility
                && self.visibility_rank(visibility)
                    < self.visibility_rank(previous.visibility.unwrap_or(VisibilityClass::Private))
            {
                violations.push(VisibilityOrderViolation::new(
                    path,
                    node.span,
                    visibility,
                    previous.label.clone(),
                    node.kind,
                ));
            }

            if highest_visibility.is_none_or(|previous| {
                self.visibility_rank(visibility)
                    > self.visibility_rank(previous.visibility.unwrap_or(VisibilityClass::Private))
            }) {
                highest_visibility = Some(node);
            }
        }
    }

    fn visibility_rank(&self, visibility: VisibilityClass) -> usize {
        self.visibility_order
            .iter()
            .position(|expected| *expected == visibility)
            .unwrap_or(self.visibility_order.len())
    }
}

impl TypedRule for VisibilityOrderRule {
    type Violation = VisibilityOrderViolation;

    fn name() -> &'static str {
        "visibility_order"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let mut violations = Vec::new();

        for items in crate::rsl::ast::module_scopes(ctx.file) {
            let nodes = crate::rsl::ast::module_nodes(items);
            let order_nodes: Vec<_> = nodes.iter().map(|node| node.order.clone()).collect();
            self.check_visibility_order(&order_nodes, ctx.path, &mut violations);

            for item in items.iter().rev() {
                if let Item::Impl(item_impl) = item
                    && item_impl.trait_.is_none()
                {
                    self.check_visibility_order(&crate::rsl::ast::impl_nodes(item_impl), ctx.path, &mut violations);
                }
            }
        }

        violations
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Serialize)]
pub(super) struct VisibilityOrderViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    message: &'static str,
    details: VisibilityDetails,
}

impl VisibilityOrderViolation {
    fn new(
        path: &Path,
        span: Span,
        actual_visibility: VisibilityClass,
        expected_before: String,
        item: ItemKind,
    ) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "visibility out of order",
            details: VisibilityDetails {
                actual_visibility,
                expected_before,
                item,
            },
        }
    }
}

impl TypedRuleViolation for VisibilityOrderViolation {
    type Rule = VisibilityOrderRule;
}

impl Display for VisibilityOrderViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            self.message,
            &format!(
                "{} -> before {} [{}]",
                self.details.actual_visibility,
                self.details.expected_before,
                self.details.item.label()
            ),
        ))
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Serialize)]
struct VisibilityDetails {
    actual_visibility: VisibilityClass,
    expected_before: String,
    item: ItemKind,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::VisibilityDetails;
    use super::VisibilityOrderRule;
    use super::VisibilityOrderViolation;
    use crate::rsl::ast::ItemKind;
    use crate::rsl::ast::VisibilityClass;
    use crate::rsl::rules::TypedRule;

    #[test]
    fn test_visibility_order_rule_check_when_visibility_decreases_reports_public_item() {
        let syntax = syn::parse_file(
            r"
            fn private() {}
            pub fn public() {}
            ",
        )
        .unwrap();

        let result = VisibilityOrderRule::new(None).check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![VisibilityOrderViolation {
                file: PathBuf::from("test.rs"),
                line: 3,
                column: 17,
                message: "visibility out of order",
                details: VisibilityDetails {
                    actual_visibility: VisibilityClass::Public,
                    expected_before: "fn private".to_owned(),
                    item: ItemKind::Fn,
                },
            }])
        );
    }

    #[test]
    fn test_visibility_order_rule_check_when_type_is_more_visible_than_previous_item_reports_type() {
        let syntax = syn::parse_file(
            r"
            fn private() {}
            pub struct Data;
            impl Data {}
            ",
        )
        .unwrap();

        let result = VisibilityOrderRule::new(None).check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![VisibilityOrderViolation {
                file: PathBuf::from("test.rs"),
                line: 3,
                column: 17,
                message: "visibility out of order",
                details: VisibilityDetails {
                    actual_visibility: VisibilityClass::Public,
                    expected_before: "fn private".to_owned(),
                    item: ItemKind::Struct,
                },
            }])
        );
    }

    #[test]
    fn test_visibility_order_rule_check_when_associated_visibility_decreases_reports_public_item() {
        let syntax = syn::parse_file(
            r"
            struct Data;
            impl Data {
                fn helper() {}
                pub fn api() {}
            }
            ",
        )
        .unwrap();

        let result = VisibilityOrderRule::new(None).check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![VisibilityOrderViolation {
                file: PathBuf::from("test.rs"),
                line: 5,
                column: 21,
                message: "visibility out of order",
                details: VisibilityDetails {
                    actual_visibility: VisibilityClass::Public,
                    expected_before: "fn helper".to_owned(),
                    item: ItemKind::Fn,
                },
            }])
        );
    }

    #[test]
    fn test_visibility_order_violation_when_details_are_present_formats_compact_output() {
        let violation = VisibilityOrderViolation {
            file: PathBuf::from("test.rs"),
            line: 3,
            column: 17,
            message: "visibility out of order",
            details: VisibilityDetails {
                actual_visibility: VisibilityClass::Public,
                expected_before: "fn private".to_owned(),
                item: ItemKind::Fn,
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:3:17 visibility out of order - pub -> before fn private [fn]"
        );
    }
}
