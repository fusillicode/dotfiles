//! Show which module an imported function belongs to when calling it.
//!
//! For example, replace `use std::fs::read_to_string;` with `use std::fs;`
//! and call `fs::read_to_string("notes.txt")` instead of `read_to_string("notes.txt")`.
//! Keep the original function import if it is still used elsewhere.
//! Also keep it when a macro or a module in another file may still need it.
//!
//! Leave locally defined functions unchanged. Also leave calls unchanged when names could conflict
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
use super::common::module_idx::has_external_modules;
use super::common::path_resolution::path_label;
use super::common::plan_unused_fn_import_removals;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct UnqualifiedCallRule;

impl Rule for UnqualifiedCallRule {
    fn id(&self) -> &'static str {
        "unqualified-call"
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
    let external_references = has_external_modules(scope.items);
    let mut changed = Vec::new();

    for call in &analysis.calls {
        if !call.is_bare() || call.is_locally_bound() {
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
        changed.push(span);
    }

    if !external_references && !analysis.opaque_expansions {
        plan_unused_fn_import_removals(ctx, scope.items, &changed, &analysis.references, plan)?;
    }

    imports.apply(ctx, scope, &mut plan.edits)
}
