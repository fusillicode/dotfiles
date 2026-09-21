//! Aliased-import rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use syn::Item;
use syn::UseTree;

use super::common::module_idx;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) struct AliasedImportViolation {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) unaliased_import: String,
}

impl AliasedImportViolation {
    pub(super) fn new(path: &Path, span: Span, unaliased_import: String) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
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

impl Display for AliasedImportViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            AliasedImportRule::code(),
            &format!("use unaliased import `{}` if it doesn't clash", self.unaliased_import),
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
}
