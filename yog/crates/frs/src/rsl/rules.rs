//! Built-in rsl rules.

use serde::Serialize;

use crate::rsl::engine::FileContext;
use crate::rsl::rules::fn_order::FnOrderRule;
use crate::rsl::rules::impl_adjacency::ImplAdjacencyRule;
use crate::rsl::rules::item_group::ItemGroupRule;
use crate::rsl::rules::qualification::QualificationRule;
use crate::rsl::rules::visibility_order::VisibilityOrderRule;

mod fn_order;
mod impl_adjacency;
mod item_group;
mod qualification;
mod visibility_order;

/// Object-safe rule interface used by the dispatcher.
///
/// Concrete rules implement [`TypedRule`]. Its blanket implementation erases
/// the concrete violation type only at this registry boundary.
trait Rule: Send + Sync {
    fn check(&self, ctx: &FileContext<'_>) -> Vec<Box<dyn RuleViolation>>;
}

impl<T> Rule for T
where
    T: crate::rsl::rules::TypedRule,
{
    fn check(&self, ctx: &FileContext<'_>) -> Vec<Box<dyn RuleViolation>> {
        <T as crate::rsl::rules::TypedRule>::check(self, ctx)
            .into_iter()
            .map(|violation| Box::new(violation) as Box<dyn RuleViolation>)
            .collect()
    }
}

/// Object-safe violation interface used after the dispatcher erases types.
trait RuleViolation: Send + Sync {
    fn to_json(&self) -> serde_json::Result<serde_json::Value>;
}

impl<T> RuleViolation for T
where
    T: crate::rsl::rules::TypedRuleViolation,
{
    fn to_json(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(SerializedViolation {
            rule: <T::Rule as crate::rsl::rules::TypedRule>::name(),
            violation: self,
        })
    }
}

/// Typed rule contract implemented by each concrete rule.
///
/// Keeping the associated violation here prevents a rule from returning the
/// violation type owned by another rule, while still allowing `dyn Rule`.
trait TypedRule: Send + Sync + 'static {
    type Violation: crate::rsl::rules::TypedRuleViolation<Rule = Self> + 'static;

    fn name() -> &'static str;

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation>;
}

/// Typed link between a concrete violation and its owning rule.
trait TypedRuleViolation: Serialize + Send + Sync + 'static {
    type Rule: crate::rsl::rules::TypedRule<Violation = Self>;
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

pub(super) fn check(ctx: &FileContext<'_>) -> serde_json::Result<Vec<serde_json::Value>> {
    let rules: Vec<Box<dyn Rule>> = vec![
        Box::new(ItemGroupRule::new(None)),
        Box::new(VisibilityOrderRule::new(None)),
        Box::new(ImplAdjacencyRule),
        Box::new(FnOrderRule),
        Box::new(QualificationRule),
    ];

    let mut violations = Vec::new();
    for rule in rules {
        for violation in rule.check(ctx) {
            violations.push(violation.to_json()?);
        }
    }

    Ok(violations)
}

#[cfg(test)]
pub(super) fn test_ctx(file: &syn::File) -> FileContext<'_> {
    FileContext {
        path: std::path::Path::new("test.rs"),
        file,
    }
}
