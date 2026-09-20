//! Nonadjacent-impl rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;

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

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug)]
pub(super) struct NonadjacentImplViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    details: NonadjacentImplDetails,
}

impl NonadjacentImplViolation {
    fn new(path: &Path, span: Span, expected_after: String, item: ItemKind) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            details: NonadjacentImplDetails { expected_after, item },
        }
    }
}

impl TypedRuleViolation for NonadjacentImplViolation {
    type Rule = NonadjacentImplRule;
}

impl Display for NonadjacentImplViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            NonadjacentImplRule::code(),
            &format!(
                "move `{}` after `{}`",
                self.details.item.label(),
                self.details.expected_after
            ),
        ))
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug)]
struct NonadjacentImplDetails {
    expected_after: String,
    item: ItemKind,
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
                file: PathBuf::from("test.rs"),
                line: 4,
                column: 13,
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
                    file: PathBuf::from("test.rs"),
                    line: 4,
                    column: 13,
                    details: NonadjacentImplDetails {
                        expected_after: "struct Data".to_owned(),
                        item: ItemKind::Impl,
                    },
                },
                NonadjacentImplViolation {
                    file: PathBuf::from("test.rs"),
                    line: 3,
                    column: 13,
                    details: NonadjacentImplDetails {
                        expected_after: "inherent impl Data".to_owned(),
                        item: ItemKind::Impl,
                    },
                },
            ])
        );
    }

    #[test]
    fn test_nonadjacent_impl_violation_when_details_are_present_formats_compact_output() {
        let violation = NonadjacentImplViolation {
            file: PathBuf::from("test.rs"),
            line: 4,
            column: 13,
            details: NonadjacentImplDetails {
                expected_after: "struct Data".to_owned(),
                item: ItemKind::Impl,
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:4:13,nonadjacent_impl,move `impl` after `struct Data`"
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
                file: PathBuf::from("test.rs"),
                line: 5,
                column: 13,
                details: NonadjacentImplDetails {
                    expected_after: "struct Data".to_owned(),
                    item: ItemKind::Impl,
                },
            }])
        );
    }
}
