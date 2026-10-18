//! Rules analyze one source snapshot, independently of the CLI mode.
//!
//! To add a rule, add its file, declare its module here, and add it to `RULES`. The file declares its
//! change data, implements `RuleChange` to describe it, and owns its skip reasons and edit planning.
//! `Rule::analyze` returns the plan; the engine alone applies edits or collects findings.
//! No rule-specific types or reason codes belong in the engine, analysis layer, or renderer.

use call_qualification_rule::CallQualificationRule;
use import_aliasing_rule::ImportAliasingRule;
use item_ordering_rule::ItemOrderingRule;
use item_qualification_rule::ItemQualificationRule;
use path_anchoring_rule::PathAnchoringRule;
use rootcause::report;

use super::analysis::RulePlan;
use super::engine::FileContext;

mod call_qualification_rule;
mod common;
mod import_aliasing_rule;
mod item_ordering_rule;
mod item_qualification_rule;
mod path_anchoring_rule;

/// Registry order resolves paths and aliases before imports and calls, then orders the resulting items.
static RULES: &[&dyn Rule] = &[
    &PathAnchoringRule,
    &ImportAliasingRule,
    &CallQualificationRule,
    &ItemQualificationRule,
    &ItemOrderingRule,
];

pub(super) trait Rule: Send + Sync {
    fn id(&self) -> &'static str;

    fn analyze<'source>(&self, ctx: &FileContext<'source>) -> rootcause::Result<RulePlan<'source>>;
}

pub(super) struct SelectedRules(Vec<&'static dyn Rule>);

impl SelectedRules {
    pub(super) fn iter(&self) -> impl Iterator<Item = &'static dyn Rule> + '_ {
        self.0.iter().copied()
    }
}

impl TryFrom<Vec<String>> for SelectedRules {
    type Error = rootcause::Report;

    fn try_from(ids: Vec<String>) -> Result<Self, Self::Error> {
        for id in &ids {
            if !RULES.iter().any(|rule| rule.id() == id) {
                let available: Vec<_> = RULES.iter().map(|rule| rule.id()).collect();
                return Err(report!("unknown rsl rule")
                    .attach(format!("rule={id}"))
                    .attach(format!("available_rules={}", available.join(", "))));
            }
        }

        Ok(Self(
            RULES
                .iter()
                .copied()
                .filter(|rule| ids.is_empty() || ids.iter().any(|id| id == rule.id()))
                .collect(),
        ))
    }
}
