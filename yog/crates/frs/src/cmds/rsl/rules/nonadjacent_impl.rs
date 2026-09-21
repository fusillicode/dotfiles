//! Nonadjacent-impl rule for `frs rsl`.

use std::path::Path;

use proc_macro2::Span;

use super::common::Location;
use crate::cmds::rsl::ast::ItemKind;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

pub struct NonadjacentImplRule;

impl TypedRule for NonadjacentImplRule {
    type Violation = NonadjacentImplViolation;

    fn code() -> &'static str {
        "nonadjacent_impl"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let mut violations = Vec::new();

        for items in &ctx.module_item_lists {
            // Raw AST positions make opaque macros and cfg-decorated items break physical adjacency.
            for node in &crate::cmds::rsl::ast::module_nodes(items) {
                if node.idxs.len() < 2 {
                    continue;
                }

                for pair in node.idxs.windows(2) {
                    let [previous, current] = pair else {
                        continue;
                    };
                    if *current == previous.saturating_add(1) {
                        continue;
                    }

                    let Some(previous_item) = items.get(*previous).map(crate::cmds::rsl::ast::ModuleItem::item) else {
                        continue;
                    };
                    let Some(classified) = items
                        .get(*current)
                        .map(crate::cmds::rsl::ast::ModuleItem::metadata)
                        .and_then(crate::cmds::rsl::ast::ItemMetadata::classified)
                    else {
                        continue;
                    };
                    violations.push(NonadjacentImplViolation::new(
                        ctx.path,
                        classified.span,
                        crate::cmds::rsl::ast::impl_order_label(previous_item),
                        classified.kind,
                    ));
                }
            }
        }

        violations
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct NonadjacentImplViolation {
    pub location: Location,
    pub details: NonadjacentImplDetails,
}

impl NonadjacentImplViolation {
    fn new(path: &Path, span: Span, expected_after: String, item: ItemKind) -> Self {
        Self {
            location: Location::from_span(path, span),
            details: NonadjacentImplDetails { expected_after, item },
        }
    }
}

impl TypedRuleViolation for NonadjacentImplViolation {
    type Rule = NonadjacentImplRule;
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct NonadjacentImplDetails {
    pub expected_after: String,
    pub item: ItemKind,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::NonadjacentImplDetails;
    use super::NonadjacentImplRule;
    use super::NonadjacentImplViolation;
    use crate::cmds::rsl::ast::ItemKind;
    use crate::cmds::rsl::rules::TypedRule;
    use crate::cmds::rsl::rules::common::Location;

    #[test]
    fn test_nonadjacent_impl_rule_check_when_impl_is_not_adjacent_to_type_reports_impl() {
        let syntax = syn::parse_file(
            r"
            struct Data;
            opaque!();
            impl Data {}
            ",
        )
        .unwrap();

        let result = NonadjacentImplRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![NonadjacentImplViolation {
                location: Location::new(PathBuf::from("test.rs"), 4, 13),
                details: NonadjacentImplDetails {
                    expected_after: "struct Data".to_owned(),
                    item: ItemKind::Impl,
                },
            }])
        );
    }

    #[test]
    fn test_nonadjacent_impl_rule_check_when_trait_impl_precedes_inherent_impl_reports_impls() {
        let syntax = syn::parse_file(
            r"
            struct Data;
            impl Behavior for Data {}
            impl Data {}
            ",
        )
        .unwrap();

        let result = NonadjacentImplRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![
                NonadjacentImplViolation {
                    location: Location::new(PathBuf::from("test.rs"), 4, 13),
                    details: NonadjacentImplDetails {
                        expected_after: "struct Data".to_owned(),
                        item: ItemKind::Impl,
                    },
                },
                NonadjacentImplViolation {
                    location: Location::new(PathBuf::from("test.rs"), 3, 13),
                    details: NonadjacentImplDetails {
                        expected_after: "inherent impl Data".to_owned(),
                        item: ItemKind::Impl,
                    },
                },
            ])
        );
    }

    #[test]
    fn test_nonadjacent_impl_rule_check_when_cfg_item_is_between_type_and_impl_reports_impl() {
        let syntax = syn::parse_file(
            r#"
            struct Data;
            #[cfg(feature = "extra")]
            fn extra() {}
            impl Data {}
            "#,
        )
        .unwrap();

        let result = NonadjacentImplRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![NonadjacentImplViolation {
                location: Location::new(PathBuf::from("test.rs"), 5, 13),
                details: NonadjacentImplDetails {
                    expected_after: "struct Data".to_owned(),
                    item: ItemKind::Impl,
                },
            }])
        );
    }
}
