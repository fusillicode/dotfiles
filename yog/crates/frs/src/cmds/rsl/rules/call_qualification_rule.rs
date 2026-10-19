//! Call imported functions through their module, using the shortest clear module name.
//!
//! For example, import `std::fs` and call `fs::read_to_string("notes.txt")`
//! instead of `read_to_string("notes.txt")` or `std::fs::read_to_string("notes.txt")`.
//!
//! Keep an existing function import when another use still needs it. Leave a call unchanged
//! when its import cannot safely be removed or its module name could mean something else.
//!
//! Call functions defined in the current module directly, such as `helper()` instead of
//! `self::helper()`, when no local name hides the function.

use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;

use proc_macro2::Span;

use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::Rule;
use crate::cmds::rsl::rules::common;
use crate::cmds::rsl::rules::common::ImportEdits;
use crate::cmds::rsl::rules::common::ImportError;
use crate::cmds::rsl::rules::common::fn_call_analysis::CallAnalysis;
use crate::cmds::rsl::rules::common::fn_path_resolution::CallTarget;
use crate::cmds::rsl::rules::common::fn_path_resolution::ModuleAccess;
use crate::cmds::rsl::rules::common::import_resolution;
use crate::cmds::rsl::rules::common::module_idx;
use crate::cmds::rsl::rules::common::module_idx::ModuleIdx;
use crate::cmds::rsl::rules::common::module_idx::ModuleScope;
use crate::cmds::rsl::rules::common::module_idx::ScopeInfo;
use crate::cmds::rsl::rules::common::path_resolution;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct CallQualificationRule;

impl Rule for CallQualificationRule {
    fn id(&self) -> &'static str {
        "call-qualification"
    }

    fn needs_module_context(&self) -> bool {
        true
    }

    fn analyze<'source>(&self, ctx: &FileContext<'source>) -> rootcause::Result<RulePlan<'source>> {
        let mut plan = RulePlan {
            edits: SourceEditor::new(ctx.source),
            findings: Vec::new(),
        };
        let idx = common::module_idx(ctx.file);
        for scope in &idx.scopes {
            analyze_scope(ctx, &idx, scope, &mut plan)?;
        }
        Ok(plan)
    }
}

struct Change {
    span: Span,
    source_path: String,
    expected_path: String,
}

impl RuleChange for Change {
    fn span(&self) -> Span {
        self.span
    }

    fn description(&self) -> String {
        format!("replace `{}` with `{}`", self.source_path, self.expected_path)
    }
}

struct CallRewrite {
    change: Change,
    target: CallTarget,
    source: CallSource,
}

enum CallSource {
    FunctionImport(usize),
    Qualified,
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
    let analysis = common::analyze_calls(scope, idx);
    let mut rewrites = collect_rewrites(ctx, idx, scope, info, &analysis, plan);
    let usage = ImportUsage {
        scope,
        info,
        analysis: &analysis,
        removable: common::removable_imports(ctx, scope.items, &plan.edits),
    };

    // Rejected calls keep their original references. Recompute dependencies until
    // every retained rewrite and every import decision agree on the final scope.
    loop {
        let removed = usage.unused_imports(&rewrites);
        if usage.reject_unsafe_removals(&removed, &mut rewrites, plan) {
            continue;
        }
        let mut imports = ImportEdits::for_calls(scope, info, &analysis, &removed);
        if let Some((index, reason)) = rewrites.iter().enumerate().find_map(|(index, rewrite)| {
            let CallTarget::Module {
                access: ModuleAccess::Import(path),
                ..
            } = &rewrite.target
            else {
                return None;
            };
            imports
                .request_call_module(path, scope)
                .err()
                .map(|reason| (index, reason))
        }) {
            let rewrite = rewrites.remove(index);
            plan.record_blocked(&rewrite.change, reason);
            continue;
        }

        for rewrite in rewrites {
            plan.edits
                .replace_span(rewrite.change.span, &rewrite.change.expected_path)?;
            plan.record_fixable(&rewrite.change);
        }
        let removed_ranges = removed
            .iter()
            .filter_map(|index| info.imports.get(*index))
            .map(|binding| plan.edits.span_range(binding.span))
            .collect();
        common::plan_import_removals(scope.items, &removed_ranges, plan)?;
        return imports.apply(ctx, scope, &mut plan.edits);
    }
}

fn collect_rewrites(
    ctx: &FileContext<'_>,
    idx: &ModuleIdx<'_>,
    scope: &ModuleScope<'_>,
    info: &ScopeInfo,
    analysis: &CallAnalysis<'_>,
    plan: &mut RulePlan<'_>,
) -> Vec<CallRewrite> {
    let mut rewrites = Vec::new();
    for call in &analysis.calls {
        if call.is_bare() && call.is_locally_bound() {
            continue;
        }
        let Some(suggestion) = common::expected_fn_path(ctx, idx, &scope.path, call.path) else {
            continue;
        };
        let source_path = path_resolution::path_label(call.path);
        if source_path == suggestion.expected_path {
            continue;
        }
        let change = Change {
            span: call.span(),
            source_path,
            expected_path: suggestion.expected_path.clone(),
        };
        if call
            .path
            .segments
            .first()
            .is_some_and(|segment| import_resolution::has_conditional_import(scope.items, &segment.ident.to_string()))
        {
            plan.record_blocked(&change, ImportError::UnknownImports);
            continue;
        }

        if call.conditional {
            plan.record_blocked(&change, common::conditional_code_reason());
            continue;
        }
        let target = match common::ensure_no_comments(&plan.edits, change.span)
            .and_then(|()| call.resolve_target(&suggestion, info).map_err(Into::into))
        {
            Ok(target) => target,
            Err(reason) => {
                plan.record_blocked(&change, reason);
                continue;
            }
        };
        let source = if call.is_bare() {
            let matching: Vec<_> = info
                .imports
                .iter()
                .enumerate()
                .filter(|(_, binding)| binding.name == change.source_path)
                .collect();
            let [(index, _)] = matching.as_slice() else {
                plan.record_blocked(&change, import_usage_reason());
                continue;
            };
            CallSource::FunctionImport(*index)
        } else {
            CallSource::Qualified
        };
        rewrites.push(CallRewrite { change, target, source });
    }
    rewrites
}

struct ImportUsage<'scope, 'ast> {
    scope: &'scope ModuleScope<'ast>,
    info: &'scope ScopeInfo,
    analysis: &'scope CallAnalysis<'ast>,
    removable: HashSet<Range<usize>>,
}

impl ImportUsage<'_, '_> {
    fn unused_imports(&self, rewrites: &[CallRewrite]) -> HashSet<usize> {
        let info = self.info;
        let analysis = self.analysis;
        let mut rewritten = HashMap::<usize, usize>::new();
        let mut needed = HashSet::new();
        for rewrite in rewrites {
            if let CallSource::FunctionImport(index) = rewrite.source {
                let count = rewritten.entry(index).or_default();
                *count = count.saturating_add(1);
            }
            if let CallTarget::Module {
                access: ModuleAccess::Imported(index),
                ..
            } = &rewrite.target
            {
                needed.insert(*index);
            }
        }
        rewritten
            .into_iter()
            .filter_map(|(index, count)| {
                let binding = info.imports.get(index)?;
                (analysis.references.values.get(&binding.name) == Some(&count)
                    && !needed.contains(&index)
                    && !analysis.references.tokens.contains(&binding.name))
                .then_some(index)
            })
            .collect()
    }

    fn reject_unsafe_removals(
        &self,
        removed: &HashSet<usize>,
        rewrites: &mut Vec<CallRewrite>,
        plan: &mut RulePlan<'_>,
    ) -> bool {
        let info = self.info;
        let analysis = self.analysis;
        // A use path's root can occupy several namespaces. Until its definition is
        // known, it cannot prove that a same-named function import stays used.
        let import_roots: HashSet<_> = rewrites
            .iter()
            .filter_map(|rewrite| {
                let CallTarget::Module {
                    access: ModuleAccess::Import(path),
                    ..
                } = &rewrite.target
                else {
                    return None;
                };
                path.strip_prefix("self::").unwrap_or(path).split("::").next()
            })
            .map(str::to_owned)
            .collect();
        let before = rewrites.len();
        rewrites.retain(|rewrite| {
            let CallSource::FunctionImport(index) = rewrite.source else {
                return true;
            };
            let Some(binding) = info.imports.get(index) else {
                return true;
            };
            if !removed.contains(&index) {
                return true;
            }
            let uncertain = module_idx::has_external_modules(self.scope.items)
                || analysis.references.uncertain.contains(&binding.name)
                || analysis.references.types.contains_key(&binding.name)
                || !self.removable.contains(&plan.edits.span_range(binding.span))
                || import_roots.contains(&binding.name);
            if uncertain {
                plan.record_blocked(&rewrite.change, import_usage_reason());
            }
            !uncertain
        });
        rewrites.len() != before
    }
}

fn import_usage_reason() -> FixBlockReason {
    FixBlockReason {
        code: "import-usage-unknown",
        message: "Changing these calls would require removing a function import whose other uses cannot be checked."
            .to_owned(),
        help: "Check child-module references, import dependencies, and comments before removing this import.",
    }
}
