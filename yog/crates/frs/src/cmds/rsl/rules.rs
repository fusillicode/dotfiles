//! Built-in rules for `frs rsl`.

use std::fmt::Display;
use std::path::Path;
use std::sync::OnceLock;

use serde::Serialize;

use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::fn_order::FnOrderRule;
use crate::cmds::rsl::rules::impl_adjacency::ImplAdjacencyRule;
use crate::cmds::rsl::rules::import_alias::ImportAliasRule;
use crate::cmds::rsl::rules::item_group::ItemGroupRule;
use crate::cmds::rsl::rules::overqualified_call::OverqualifiedCallRule;
use crate::cmds::rsl::rules::qualified_item_path::QualifiedItemPathRule;
use crate::cmds::rsl::rules::unqualified_call::UnqualifiedCallRule;
use crate::cmds::rsl::rules::visibility_order::VisibilityOrderRule;

mod common;
mod fn_order;
mod impl_adjacency;
mod import_alias;
mod item_group;
mod overqualified_call;
mod qualified_item_path;
mod unqualified_call;
mod visibility_order;

static RULES: OnceLock<[Box<dyn Rule>; 8]> = OnceLock::new();

/// Object-safe rule interface used by the dispatcher.
///
/// Concrete rules implement [`TypedRule`]. Its blanket implementation erases
/// the concrete violation type only at this registry boundary.
trait Rule: Send + Sync {
    fn check(&self, ctx: &FileContext<'_>) -> Vec<Box<dyn RuleViolation>>;
}

impl<T> Rule for T
where
    T: crate::cmds::rsl::rules::TypedRule,
{
    fn check(&self, ctx: &FileContext<'_>) -> Vec<Box<dyn RuleViolation>> {
        <T as crate::cmds::rsl::rules::TypedRule>::check(self, ctx)
            .into_iter()
            .map(|violation| Box::new(violation) as Box<dyn RuleViolation>)
            .collect()
    }
}

/// Object-safe violation interface used after the dispatcher erases types.
pub trait RuleViolation: Display + Send + Sync {
    fn write_json(&self, output: &mut Vec<u8>) -> serde_json::Result<()>;
}

impl<T> RuleViolation for T
where
    T: crate::cmds::rsl::rules::TypedRuleViolation + Display,
{
    fn write_json(&self, output: &mut Vec<u8>) -> serde_json::Result<()> {
        serde_json::to_writer(
            output,
            &SerializedViolation {
                rule: <T::Rule as crate::cmds::rsl::rules::TypedRule>::name(),
                violation: self,
            },
        )
    }
}

/// Typed rule contract implemented by each concrete rule.
///
/// Keeping the associated violation here prevents a rule from returning the
/// violation type owned by another rule, while still allowing `dyn Rule`.
trait TypedRule: Send + Sync + 'static {
    type Violation: crate::cmds::rsl::rules::TypedRuleViolation<Rule = Self> + Display + 'static;

    fn name() -> &'static str;

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation>;
}

/// Typed link between a concrete violation and its owning rule.
trait TypedRuleViolation: Serialize + Send + Sync + 'static {
    type Rule: crate::cmds::rsl::rules::TypedRule<Violation = Self>;
}

pub(super) fn format_compact_violation(
    file: &Path,
    line: usize,
    column: usize,
    message: &str,
    details: &str,
) -> String {
    if details.is_empty() {
        format!("{}:{line}:{column} {message}", file.display())
    } else {
        format!("{}:{line}:{column} {message} - {details}", file.display())
    }
}

/// Serialization-only adapter for the CLI output.
///
/// Concrete rules keep returning their own violation types. This adapter is
/// needed only when the dispatcher combines those different types into one
/// JSON list. The concrete violation selects its rule name through its
/// associated `TypedRule` implementation.
#[derive(Serialize)]
struct SerializedViolation<'rule, V: ?Sized> {
    rule: &'rule str,
    #[serde(flatten)]
    violation: &'rule V,
}

pub(super) fn check(ctx: &FileContext<'_>) -> Vec<Box<dyn RuleViolation>> {
    let mut violations = Vec::new();
    for rule in self::rules() {
        violations.extend(rule.check(ctx));
    }

    violations
}

fn rules() -> &'static [Box<dyn Rule>] {
    RULES
        .get_or_init(|| {
            [
                Box::new(ItemGroupRule::new(None)) as Box<dyn Rule>,
                Box::new(VisibilityOrderRule::new(None)),
                Box::new(ImplAdjacencyRule),
                Box::new(FnOrderRule),
                Box::new(UnqualifiedCallRule),
                Box::new(OverqualifiedCallRule),
                Box::new(QualifiedItemPathRule),
                Box::new(ImportAliasRule),
            ]
        })
        .as_slice()
}

#[cfg(test)]
pub(super) fn test_ctx(file: &syn::File) -> FileContext<'_> {
    FileContext {
        path: std::path::Path::new("test.rs"),
        file,
        module_item_lists: crate::cmds::rsl::ast::module_item_lists(file),
    }
}
