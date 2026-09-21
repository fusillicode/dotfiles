//! Misordered-visibility rule for `frs rsl`.

use std::path::Path;

use proc_macro2::Span;
use syn::Item;

use super::common::Location;
use crate::cmds::rsl::ast::VisibilityClass;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

pub struct MisorderedVisibilityRule {
    visibility_order: [VisibilityClass; 4],
}

impl MisorderedVisibilityRule {
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
        nodes: &[crate::cmds::rsl::ast::OrderNode],
        path: &Path,
        violations: &mut Vec<MisorderedVisibilityViolation>,
    ) {
        let mut current_group = None;
        let mut highest_visibility: Option<&crate::cmds::rsl::ast::OrderNode> = None;

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
                violations.push(MisorderedVisibilityViolation::new(
                    path,
                    node.span,
                    previous.label.clone(),
                    node.label.clone(),
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

impl TypedRule for MisorderedVisibilityRule {
    type Violation = MisorderedVisibilityViolation;

    fn code() -> &'static str {
        "misordered_visibility"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let mut violations = Vec::new();

        for items in &ctx.module_item_lists {
            let nodes = crate::cmds::rsl::ast::module_nodes(items);
            let order_nodes: Vec<_> = nodes.iter().map(|node| node.order.clone()).collect();
            self.check_visibility_order(&order_nodes, ctx.path, &mut violations);

            for module_item in items.iter().rev() {
                let item = module_item.item();
                if let Item::Impl(item_impl) = item
                    && item_impl.trait_.is_none()
                {
                    self.check_visibility_order(
                        &crate::cmds::rsl::ast::impl_nodes(item_impl),
                        ctx.path,
                        &mut violations,
                    );
                }
            }
        }

        violations
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct MisorderedVisibilityViolation {
    pub location: Location,
    pub details: MisorderedVisibilityDetails,
}

impl MisorderedVisibilityViolation {
    fn new(path: &Path, span: Span, expected_before: String, item_label: String) -> Self {
        Self {
            location: Location::from_span(path, span),
            details: MisorderedVisibilityDetails {
                expected_before,
                item_label,
            },
        }
    }
}

impl TypedRuleViolation for MisorderedVisibilityViolation {
    type Rule = MisorderedVisibilityRule;
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct MisorderedVisibilityDetails {
    pub expected_before: String,
    pub item_label: String,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::MisorderedVisibilityDetails;
    use super::MisorderedVisibilityRule;
    use super::MisorderedVisibilityViolation;
    use crate::cmds::rsl::rules::TypedRule;
    use crate::cmds::rsl::rules::common::Location;

    #[test]
    fn test_misordered_visibility_rule_check_when_visibility_decreases_reports_public_item() {
        let syntax = syn::parse_file(
            r"
            fn private() {}
            pub fn public() {}
            ",
        )
        .unwrap();

        let result = MisorderedVisibilityRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedVisibilityViolation {
                location: Location::new(PathBuf::from("test.rs"), 3, 17),
                details: MisorderedVisibilityDetails {
                    expected_before: "fn private".to_owned(),
                    item_label: "fn public".to_owned(),
                },
            }])
        );
    }

    #[test]
    fn test_misordered_visibility_rule_check_when_type_is_more_visible_than_previous_item_reports_type() {
        let syntax = syn::parse_file(
            r"
            fn private() {}
            pub struct Data;
            impl Data {}
            ",
        )
        .unwrap();

        let result = MisorderedVisibilityRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedVisibilityViolation {
                location: Location::new(PathBuf::from("test.rs"), 3, 17),
                details: MisorderedVisibilityDetails {
                    expected_before: "fn private".to_owned(),
                    item_label: "struct Data".to_owned(),
                },
            }])
        );
    }

    #[test]
    fn test_misordered_visibility_rule_check_when_associated_visibility_decreases_reports_public_item() {
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

        let result = MisorderedVisibilityRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedVisibilityViolation {
                location: Location::new(PathBuf::from("test.rs"), 5, 21),
                details: MisorderedVisibilityDetails {
                    expected_before: "fn helper".to_owned(),
                    item_label: "fn api".to_owned(),
                },
            }])
        );
    }
}
