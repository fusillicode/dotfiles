//! Aliased-import rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use syn::Item;
use syn::UseTree;

use super::common::module_index;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) struct AliasedImportViolation {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) column: usize,
}

impl AliasedImportViolation {
    pub(super) fn new(path: &Path, span: Span) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
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
        let index = module_index(ctx.file);
        let mut violations = Vec::new();

        for scope in &index.scopes {
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
    let mut pending = vec![tree];

    while let Some(tree) = pending.pop() {
        match tree {
            UseTree::Path(path) => pending.push(path.tree.as_ref()),
            UseTree::Group(group) => pending.extend(group.items.iter()),
            UseTree::Rename(rename) if rename.rename != "_" => {
                violations.push(AliasedImportViolation::new(path, rename.rename.span()));
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
            "use unaliased import if there are no clashes",
        ))
    }
}
