//! Aliased-import rule for `frs rsl`.

use std::path::Path;

use proc_macro2::Span;
use syn::Item;
use syn::UseTree;

use super::common::Location;
use super::common::module_idx;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct AliasedImportViolation {
    pub location: Location,
    pub unaliased_import: String,
}

impl AliasedImportViolation {
    pub(super) fn new(path: &Path, span: Span, unaliased_import: String) -> Self {
        Self {
            location: Location::from_span(path, span),
            unaliased_import,
        }
    }
}

pub struct AliasedImportRule;

impl TypedRule for AliasedImportRule {
    type Violation = AliasedImportViolation;

    fn code() -> &'static str {
        "aliased_import"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let idx = module_idx(ctx.file);
        let mut violations = Vec::new();

        for scope in &idx.scopes {
            for item in scope.items {
                if let Item::Use(item_use) = item {
                    check_aliases(ctx.path, &item_use.tree, &mut violations);
                }
            }
        }

        violations
    }
}

fn check_aliases(path: &std::path::Path, tree: &UseTree, violations: &mut Vec<AliasedImportViolation>) {
    let mut pending = vec![(tree, Vec::new())];

    while let Some((tree, prefix)) = pending.pop() {
        match tree {
            UseTree::Path(use_path) => {
                let mut next_prefix = prefix;
                next_prefix.push(use_path.ident.to_string());
                pending.push((use_path.tree.as_ref(), next_prefix));
            }
            UseTree::Group(group) => {
                for tree in group.items.iter().rev() {
                    pending.push((tree, prefix.clone()));
                }
            }
            UseTree::Rename(rename) if rename.rename != "_" => {
                let unaliased_import = if rename.ident == "self" {
                    prefix.join("::")
                } else {
                    let mut import_path = prefix;
                    import_path.push(rename.ident.to_string());
                    import_path.join("::")
                };
                violations.push(AliasedImportViolation::new(
                    path,
                    rename.rename.span(),
                    unaliased_import,
                ));
            }
            UseTree::Name(_) | UseTree::Glob(_) | UseTree::Rename(_) => {}
        }
    }
}

impl TypedRuleViolation for AliasedImportViolation {
    type Rule = AliasedImportRule;
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::*;
    use crate::cmds::rsl::rules::TypedRule;
    use crate::cmds::rsl::rules::common::Location;

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
                location: Location::new(PathBuf::from("test.rs"), 2, 38),
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
                location: Location::new(PathBuf::from("test.rs"), 2, 40),
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
                location: Location::new(PathBuf::from("test.rs"), 2, 47),
                unaliased_import: "std::fmt::Debug".to_owned(),
            }])
        );
    }
}
