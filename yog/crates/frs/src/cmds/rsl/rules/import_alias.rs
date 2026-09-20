//! Import-alias rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use serde::Serialize;
use syn::Item;
use syn::UseTree;

use super::common::module_index;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
pub(super) struct ImportAliasViolation {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) message: &'static str,
    pub(super) details: ImportAliasDetails,
}

impl ImportAliasViolation {
    pub(super) fn new(path: &Path, span: Span, alias: String) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "alias not allowed",
            details: ImportAliasDetails { alias },
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
pub(super) struct ImportAliasDetails {
    pub(super) alias: String,
}

pub struct ImportAliasRule;

impl TypedRule for ImportAliasRule {
    type Violation = ImportAliasViolation;

    fn name() -> &'static str {
        "import_alias"
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

fn check_aliases(path: &std::path::Path, tree: &UseTree, violations: &mut Vec<ImportAliasViolation>) {
    let mut pending = vec![tree];

    while let Some(tree) = pending.pop() {
        match tree {
            UseTree::Path(path) => pending.push(path.tree.as_ref()),
            UseTree::Group(group) => pending.extend(group.items.iter()),
            UseTree::Rename(rename) if rename.rename != "_" => {
                violations.push(ImportAliasViolation::new(
                    path,
                    rename.rename.span(),
                    rename.rename.to_string(),
                ));
            }
            UseTree::Name(_) | UseTree::Glob(_) | UseTree::Rename(_) => {}
        }
    }
}

impl TypedRuleViolation for ImportAliasViolation {
    type Rule = ImportAliasRule;
}

impl Display for ImportAliasViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            self.message,
            &format!("alias: {} [allowed: as _]", self.details.alias),
        ))
    }
}
