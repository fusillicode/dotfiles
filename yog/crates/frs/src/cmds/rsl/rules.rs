//! Built-in rules for `frs rsl`.

use std::fmt::Display;
use std::path::Path;
use std::sync::OnceLock;

use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::aliased_import::AliasedImportRule;
use crate::cmds::rsl::rules::misordered_fn::MisorderedFnRule;
use crate::cmds::rsl::rules::misordered_item_group::MisorderedItemGroupRule;
use crate::cmds::rsl::rules::misordered_visibility::MisorderedVisibilityRule;
use crate::cmds::rsl::rules::nonadjacent_impl::NonadjacentImplRule;
use crate::cmds::rsl::rules::overqualified_call::OverqualifiedCallRule;
use crate::cmds::rsl::rules::qualified_item::QualifiedItemRule;
use crate::cmds::rsl::rules::relative_path::RelativePathRule;
use crate::cmds::rsl::rules::unqualified_call::UnqualifiedCallRule;

mod aliased_import;
mod common;
mod misordered_fn;
mod misordered_item_group;
mod misordered_visibility;
mod nonadjacent_impl;
mod overqualified_call;
mod qualified_item;
mod relative_path;
mod unqualified_call;

static RULES: OnceLock<[Box<dyn Rule>; 9]> = OnceLock::new();

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
pub trait RuleViolation: Display + Send + Sync {}

impl<T> RuleViolation for T where T: crate::cmds::rsl::rules::TypedRuleViolation + Display {}

/// Typed rule contract implemented by each concrete rule.
///
/// Keeping the associated violation here prevents a rule from returning the
/// violation type owned by another rule, while still allowing `dyn Rule`.
trait TypedRule: Send + Sync + 'static {
    type Violation: crate::cmds::rsl::rules::TypedRuleViolation<Rule = Self> + Display + 'static;

    fn code() -> &'static str;

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation>;
}

/// Typed link between a concrete violation and its owning rule.
trait TypedRuleViolation: Send + Sync + 'static {
    type Rule: crate::cmds::rsl::rules::TypedRule<Violation = Self>;
}

pub(super) fn format_compact_violation(
    file: &Path,
    line: usize,
    column: usize,
    code: &str,
    suggested_fix: &str,
) -> String {
    format!("{}:{line}:{column},{code},{suggested_fix}", file.display())
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
                Box::new(MisorderedItemGroupRule::new(None)) as Box<dyn Rule>,
                Box::new(MisorderedVisibilityRule::new(None)),
                Box::new(NonadjacentImplRule),
                Box::new(MisorderedFnRule),
                Box::new(UnqualifiedCallRule),
                Box::new(OverqualifiedCallRule),
                Box::new(QualifiedItemRule::new(
                    crate::cmds::rsl::rules::qualified_item::QUALIFIED_ALLOWED_PATHS,
                )),
                Box::new(AliasedImportRule),
                Box::new(RelativePathRule),
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
