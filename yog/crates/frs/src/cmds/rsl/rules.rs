//! Direct source fixes. Each supplied file is rewritten independently, including its inline modules.

use aliased_import_rule::AliasedImportRule;
use ordering_rule::OrderingRule;
use overqualified_call_rule::OverqualifiedCallRule;
use qualified_item_rule::QualifiedItemRule;
use relative_path_rule::RelativePathRule;
use rootcause::report;
use unqualified_call_rule::UnqualifiedCallRule;

use super::engine::FileContext;
use super::source::SourceEditor;

mod aliased_import_rule;
mod common;
mod ordering_rule;
mod overqualified_call_rule;
mod qualified_item_rule;
mod relative_path_rule;
mod unqualified_call_rule;

/// Registry order resolves paths and aliases before imports and calls, then orders the resulting items.
static RULES: [&dyn Rule; 6] = [
    &RelativePathRule,
    &AliasedImportRule,
    &UnqualifiedCallRule,
    &OverqualifiedCallRule,
    &QualifiedItemRule,
    &OrderingRule,
];

pub(super) trait Rule: Send + Sync {
    fn id(&self) -> &'static str;

    fn fix(&self, ctx: &FileContext<'_>, editor: &mut SourceEditor<'_>) -> rootcause::Result<()>;
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
