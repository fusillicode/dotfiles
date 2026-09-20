//! Misordered-item-group rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;

use crate::cmds::rsl::ast::ItemGroup;
use crate::cmds::rsl::ast::ItemKind;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

pub struct MisorderedItemGroupRule {
    group_order: [ItemGroup; 7],
}

impl MisorderedItemGroupRule {
    pub(super) fn new(group_order: Option<[ItemGroup; 7]>) -> Self {
        Self {
            group_order: group_order.unwrap_or([
                ItemGroup::ExternCrate,
                ItemGroup::Use,
                ItemGroup::Modules,
                ItemGroup::GlobalAsm,
                ItemGroup::Constants,
                ItemGroup::Aliases,
                ItemGroup::Items,
            ]),
        }
    }

    fn group_rank(&self, group: ItemGroup) -> usize {
        self.group_order
            .iter()
            .position(|expected| *expected == group)
            .unwrap_or(self.group_order.len())
    }
}

impl TypedRule for MisorderedItemGroupRule {
    type Violation = MisorderedItemGroupViolation;

    fn code() -> &'static str {
        "misordered_item_group"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let mut violations = Vec::new();

        for items in &ctx.module_item_lists {
            // Keep unknown macro invocations transparent here to preserve the original group rule.
            let mut previous_group: Option<ItemGroup> = None;

            for (index, module_item) in items.iter().enumerate() {
                let metadata = module_item.metadata();
                let Some(classified) = metadata.classified() else {
                    continue;
                };
                if metadata.is_test_module() {
                    if index != items.len().saturating_sub(1) {
                        violations.push(MisorderedItemGroupViolation::new(
                            ctx.path,
                            classified.span,
                            ItemGroup::Items,
                            classified.kind,
                        ));
                    }
                    continue;
                }

                let actual_group = classified.kind.group();
                if let Some(expected_group) = previous_group
                    && self.group_rank(actual_group) < self.group_rank(expected_group)
                {
                    violations.push(MisorderedItemGroupViolation::new(
                        ctx.path,
                        classified.span,
                        expected_group,
                        classified.kind,
                    ));
                }
                previous_group = Some(actual_group);
            }
        }

        violations
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug)]
pub(super) struct MisorderedItemGroupViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    details: MisorderedItemGroupDetails,
}

impl MisorderedItemGroupViolation {
    fn new(path: &Path, span: Span, expected_group: ItemGroup, item: ItemKind) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            details: MisorderedItemGroupDetails { expected_group, item },
        }
    }
}

impl TypedRuleViolation for MisorderedItemGroupViolation {
    type Rule = MisorderedItemGroupRule;
}

impl Display for MisorderedItemGroupViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            MisorderedItemGroupRule::code(),
            &format!(
                "move `{}` after `{}`",
                self.details.item.label(),
                self.details.expected_group
            ),
        ))
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug)]
struct MisorderedItemGroupDetails {
    expected_group: ItemGroup,
    item: ItemKind,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::MisorderedItemGroupDetails;
    use super::MisorderedItemGroupRule;
    use super::MisorderedItemGroupViolation;
    use crate::cmds::rsl::ast::ItemGroup;
    use crate::cmds::rsl::rules::TypedRule;

    #[test]
    fn test_classify_item_when_each_group_is_present_returns_expected_groups() {
        assert_that!(
            groups(
                r#"
                extern crate alloc;
                #[cfg(feature = "imports")]
                use alloc::vec::Vec;
                extern "C" {}
                #[rsl_test]
                mod child {}
                global_asm!("");
                const VALUE: usize = 1;
                static OTHER: usize = 2;
                type Alias = usize;
                macro_rules! declared {}
                enum Kind {}
                struct Data;
                union Storage { value: usize }
                trait Behavior {}
                trait AliasTrait = Behavior;
                impl Data {}
                fn run() {}
                opaque!();
                "#
            ),
            eq(vec![
                Some(ItemGroup::ExternCrate),
                Some(ItemGroup::Use),
                Some(ItemGroup::Modules),
                Some(ItemGroup::Modules),
                Some(ItemGroup::GlobalAsm),
                Some(ItemGroup::Constants),
                Some(ItemGroup::Constants),
                Some(ItemGroup::Aliases),
                Some(ItemGroup::Items),
                Some(ItemGroup::Items),
                Some(ItemGroup::Items),
                Some(ItemGroup::Items),
                Some(ItemGroup::Items),
                Some(ItemGroup::Items),
                Some(ItemGroup::Items),
                Some(ItemGroup::Items),
                None,
            ])
        );
    }

    #[test]
    fn test_misordered_item_group_rule_check_when_group_rank_decreases_reports_offending_item() {
        let syntax = syn::parse_file(
            r"
            const VALUE: usize = 1;
            fn run() {}
            use std::fmt;
            ",
        )
        .unwrap();

        let result = MisorderedItemGroupRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedItemGroupViolation {
                file: PathBuf::from("test.rs"),
                line: 4,
                column: 13,
                details: MisorderedItemGroupDetails {
                    expected_group: ItemGroup::Items,
                    item: crate::cmds::rsl::ast::ItemKind::Use,
                },
            }])
        );
    }

    #[test]
    fn test_misordered_item_group_rule_check_when_same_group_repeats_preserves_clean_order() {
        let syntax = syn::parse_file(
            r"
            const FIRST: usize = 1;
            static SECOND: usize = 2;
            fn run() {}
            fn stop() {}
            ",
        )
        .unwrap();

        let result = MisorderedItemGroupRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_misordered_item_group_rule_check_when_cfg_test_tests_module_is_last_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn run() {}

            #[cfg(test)]
            mod tests {}
            ",
        )
        .unwrap();

        let result = MisorderedItemGroupRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_misordered_item_group_rule_check_when_cfg_test_tests_module_is_not_last_reports_violation() {
        let syntax = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {}

            fn run() {}
            ",
        )
        .unwrap();

        let result = MisorderedItemGroupRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedItemGroupViolation {
                file: PathBuf::from("test.rs"),
                line: 3,
                column: 13,
                details: MisorderedItemGroupDetails {
                    expected_group: ItemGroup::Items,
                    item: crate::cmds::rsl::ast::ItemKind::Mod,
                },
            }])
        );
    }

    #[test]
    fn test_misordered_item_group_rule_check_when_opaque_macro_is_between_items_ignores_macro_barrier() {
        let syntax = syn::parse_file(
            r"
            const VALUE: usize = 1;
            opaque!();
            fn run() {}
            ",
        )
        .unwrap();

        let result = MisorderedItemGroupRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_misordered_item_group_rule_check_when_explicit_macro_definition_precedes_constant_reports_violation() {
        let syntax = syn::parse_file(
            r"
            macro_rules! declared {}
            const VALUE: usize = 1;
            ",
        )
        .unwrap();

        let result = MisorderedItemGroupRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedItemGroupViolation {
                file: PathBuf::from("test.rs"),
                line: 3,
                column: 13,
                details: MisorderedItemGroupDetails {
                    expected_group: ItemGroup::Items,
                    item: crate::cmds::rsl::ast::ItemKind::Const,
                },
            }])
        );
    }

    #[test]
    fn test_classify_item_when_declarative_macro_uses_macro_keyword_returns_items_group() {
        assert_that!(
            groups(
                r"
                macro declared {}
                "
            ),
            eq(vec![Some(ItemGroup::Items)])
        );
    }

    #[test]
    fn test_misordered_item_group_rule_check_when_inline_module_contains_violation_reports_nested_item() {
        let syntax = syn::parse_file(
            r"
            mod child {
                fn run() {}
                const VALUE: usize = 1;
            }
            ",
        )
        .unwrap();

        let result = MisorderedItemGroupRule::new(None).check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedItemGroupViolation {
                file: PathBuf::from("test.rs"),
                line: 4,
                column: 17,
                details: MisorderedItemGroupDetails {
                    expected_group: ItemGroup::Items,
                    item: crate::cmds::rsl::ast::ItemKind::Const,
                },
            }])
        );
    }

    #[test]
    fn test_misordered_item_group_violation_when_details_are_present_formats_compact_output() {
        let violation = MisorderedItemGroupViolation {
            file: PathBuf::from("test.rs"),
            line: 4,
            column: 13,
            details: MisorderedItemGroupDetails {
                expected_group: ItemGroup::Items,
                item: crate::cmds::rsl::ast::ItemKind::Const,
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:4:13,misordered_item_group,move `const` after `items`"
        );
    }

    fn groups(source: &str) -> Vec<Option<crate::cmds::rsl::ast::ItemGroup>> {
        syn::parse_file(source)
            .unwrap()
            .items
            .iter()
            .map(crate::cmds::rsl::ast::classify_item)
            .map(|item| item.map(|item| item.kind.group()))
            .collect()
    }
}
