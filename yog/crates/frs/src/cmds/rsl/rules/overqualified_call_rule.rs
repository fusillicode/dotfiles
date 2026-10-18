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
use super::common::ImportError;
use super::common::calls_in_scope;
use super::common::ensure_no_comments;
use super::common::expected_fn_path;
use super::common::local_names;
use super::common::module_idx;
use super::common::module_idx::ModuleIdx;
use super::common::module_idx::ModuleScope;
use super::common::path_resolution::path_label;
use crate::cmds::rsl::analysis::FixBlockReason;
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

enum BlockReason {
    MissingModuleContext,
}

impl From<BlockReason> for FixBlockReason {
    fn from(reason: BlockReason) -> Self {
        match reason {
            BlockReason::MissingModuleContext => Self {
                code: "parent-module-unknown",
                message: "The required crate or parent-module context is outside this file.".to_owned(),
                help: "Use the correct crate:: path after checking the surrounding modules.",
            },
        }
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

    let mut imports = ImportEdits::new(scope, info);
    let reserved = local_names(scope.items);

    for call in calls_in_scope(scope.items) {
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

        let module_name = suggestion.expected_path.split("::").next().unwrap_or_default();

        let reason = if info.unknown_imports {
            Some(ImportError::UnknownImports.into())
        } else if suggestion.requires_crate_context(source_path) {
            Some(FixBlockReason::from(BlockReason::MissingModuleContext))
        } else if reserved.has_unknown_bindings() {
            Some(ImportError::UnknownBindings.into())
        } else if reserved.contains(module_name) {
            Some(
                ImportError::NameConflict {
                    name: module_name.to_owned(),
                }
                .into(),
            )
        } else {
            None
        };

        if let Some(reason) = reason {
            plan.record_blocked(&change, reason);
            continue;
        }

        if let Some(import) = &suggestion.required_import
            && let Err(reason) = imports.request(import)
        {
            plan.record_blocked(&change, reason);
            continue;
        }

        change.add_to_plan(plan)?;
    }

    imports.apply(ctx, scope, &mut plan.edits)
}
