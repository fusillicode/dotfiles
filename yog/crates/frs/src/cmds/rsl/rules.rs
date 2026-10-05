//! Built-in rules for `frs rsl`.
//!
//! Each supplied file is checked as a root module. Inline module bodies are checked recursively;
//! external module files are not read unless supplied separately.
//!
//! The rules check item groups, visibility order, impl adjacency, function order, unqualified calls,
//! overqualified calls, qualified item paths, import aliases, and relative paths. Function calls use
//! the shortest safe module path, qualified non-function paths use imports, and import aliases are
//! limited to `as _`.
//!
//! [`ordering_rule`] replaces the four previous ordering rules; their CLI selectors are removed.

use std::iter::Copied;
use std::slice::Iter;
use std::sync::OnceLock;

use rootcause::report;

use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::output::FormattedRuleViolation;
use crate::cmds::rsl::output::ViolationOutputFormat;
use crate::cmds::rsl::rules::aliased_import::AliasedImportRule;
use crate::cmds::rsl::rules::ordering_rule::OrderingRule;
use crate::cmds::rsl::rules::overqualified_call::OverqualifiedCallRule;
use crate::cmds::rsl::rules::qualified_item::QualifiedItemRule;
use crate::cmds::rsl::rules::relative_path::RelativePathRule;
use crate::cmds::rsl::rules::unqualified_call::UnqualifiedCallRule;

pub(super) mod aliased_import;
pub(super) mod common;
pub(super) mod ordering_rule;
pub(super) mod overqualified_call;
pub(super) mod qualified_item;
pub(super) mod relative_path;
pub(super) mod unqualified_call;

static RULES: OnceLock<[Box<dyn Rule>; 6]> = OnceLock::new();

/// Object-safe rule interface used by the dispatcher.
///
/// Concrete rules implement [`TypedRule`]. Its blanket implementation erases
/// the concrete violation type only at this registry boundary.
pub(super) trait Rule: Send + Sync {
    fn code(&self) -> &'static str;

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Box<dyn RuleViolation>>;
}

impl<T> Rule for T
where
    T: crate::cmds::rsl::rules::TypedRule,
{
    fn code(&self) -> &'static str {
        T::code()
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Box<dyn RuleViolation>> {
        <T as crate::cmds::rsl::rules::TypedRule>::check(self, ctx)
            .into_iter()
            .map(|violation| Box::new(violation) as Box<dyn RuleViolation>)
            .collect()
    }
}

/// Object-safe violation interface used after the dispatcher erases types.
pub(super) trait RuleViolation: Send + Sync {
    fn render(&self, format: ViolationOutputFormat) -> String;
}

impl<T> RuleViolation for T
where
    T: crate::cmds::rsl::rules::TypedRuleViolation + FormattedRuleViolation,
{
    fn render(&self, format: ViolationOutputFormat) -> String {
        FormattedRuleViolation::format(self, format)
    }
}

/// Typed rule contract implemented by each concrete rule.
///
/// Keeping the associated violation here prevents a rule from returning the
/// violation type owned by another rule, while still allowing `dyn Rule`.
pub(super) trait TypedRule: Send + Sync + 'static {
    type Violation: crate::cmds::rsl::rules::TypedRuleViolation<Rule = Self> + FormattedRuleViolation + 'static;

    fn code() -> &'static str;

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation>;
}

/// Typed link between a concrete violation and its owning rule.
pub(super) trait TypedRuleViolation: Send + Sync + 'static {
    type Rule: crate::cmds::rsl::rules::TypedRule<Violation = Self>;
}

/// Validated built-in rules to execute, in registry order without duplicates.
pub(super) struct SelectedRules(Vec<&'static dyn Rule>);

impl TryFrom<Vec<String>> for SelectedRules {
    type Error = rootcause::Report;

    fn try_from(rule_ids: Vec<String>) -> Result<Self, Self::Error> {
        let registry: Vec<_> = self::rules()
            .iter()
            .map(|rule| (rule.code().replace('_', "-"), rule.as_ref()))
            .collect();

        for rule_id in &rule_ids {
            if !registry.iter().any(|(id, _)| id == rule_id) {
                let available: Vec<_> = registry.iter().map(|(id, _)| id.as_str()).collect();
                return Err(report!("unknown rsl rule")
                    .attach(format!("rule={rule_id}"))
                    .attach(format!("available_rules={}", available.join(", "))));
            }
        }

        let rules = registry
            .iter()
            .filter(|(id, _)| rule_ids.is_empty() || rule_ids.contains(id))
            .map(|(_, rule)| *rule)
            .collect();

        Ok(Self(rules))
    }
}

impl<'a> IntoIterator for &'a SelectedRules {
    type IntoIter = Copied<Iter<'a, Self::Item>>;
    type Item = &'static dyn Rule;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().copied()
    }
}

#[cfg(test)]
pub(super) fn test_ctx(file: &syn::File) -> FileContext<'_> {
    FileContext {
        path: std::path::Path::new("test.rs"),
        source: "",
        file,
    }
}

fn rules() -> &'static [Box<dyn Rule>] {
    RULES
        .get_or_init(|| {
            [
                Box::new(OrderingRule) as Box<dyn Rule>,
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
