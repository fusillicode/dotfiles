//! Shorten function calls while keeping the module name that shows where the function comes from.
//!
//! For example, import `std::fs` and write `fs::read_to_string("notes.txt")`
//! instead of `std::fs::read_to_string("notes.txt")`.
//!
//! Leave the call unchanged when the shorter name could refer to something else,
//! or when this file does not provide enough information to check the change.

use proc_macro2::Span;

use super::Rule;
use super::common::ImportEdits;
use super::common::analyze_calls;
use super::common::ensure_no_comments;
use super::common::expected_fn_path;
use super::common::module_idx;
use super::common::module_idx::ModuleIdx;
use super::common::module_idx::ModuleScope;
use super::common::path_resolution::path_label;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct OverqualifiedCallRule;

impl Rule for OverqualifiedCallRule {
    fn id(&self) -> &'static str {
        "overqualified-call"
    }

    fn analyze<'source>(&self, ctx: &FileContext<'source>) -> rootcause::Result<RulePlan<'source>> {
        let mut plan = RulePlan {
            edits: SourceEditor::new(ctx.source),
            findings: Vec::new(),
        };
        let idx = module_idx(ctx.file);

        for scope in &idx.scopes {
            analyze_scope(ctx, &idx, scope, &mut plan)?;
        }

        Ok(plan)
    }
}

struct Change<'path> {
    span: Span,
    source_path: &'path str,
    expected_path: &'path str,
}

impl Change<'_> {
    fn add_to_plan(&self, plan: &mut RulePlan<'_>) -> rootcause::Result<()> {
        plan.edits.replace_span(self.span, self.expected_path)?;
        plan.record_fixable(self);
        Ok(())
    }
}

impl RuleChange for Change<'_> {
    fn span(&self) -> Span {
        self.span
    }

    fn description(&self) -> String {
        format!("replace `{}` with `{}`", self.source_path, self.expected_path)
    }
}

fn analyze_scope(
    ctx: &FileContext<'_>,
    idx: &ModuleIdx<'_>,
    scope: &ModuleScope<'_>,
    plan: &mut RulePlan<'_>,
) -> rootcause::Result<()> {
    let Some(info) = idx.info.get(&scope.path) else {
        return Ok(());
    };

    let analysis = analyze_calls(scope, idx);
    let mut imports = ImportEdits::for_calls(scope, info, &analysis);

    for call in &analysis.calls {
        if call.is_bare() {
            continue;
        }

        let Some(suggestion) = expected_fn_path(idx, &scope.path, call.path) else {
            continue;
        };

        if path_label(call.path) == suggestion.expected_path {
            continue;
        }

        let span = call.span();

        let source_path = ctx.source.get(plan.edits.span_range(span)).unwrap_or_default();
        let change = Change {
            span,
            source_path,
            expected_path: &suggestion.expected_path,
        };

        if let Err(reason) = ensure_no_comments(&plan.edits, span) {
            plan.record_blocked(&change, reason);
            continue;
        }

        if let Err(reason) = call.prepare_rewrite(&suggestion, scope, info, &mut imports) {
            plan.record_blocked(&change, reason);
            continue;
        }

        change.add_to_plan(plan)?;
    }

    imports.apply(ctx, scope, &mut plan.edits)
}
