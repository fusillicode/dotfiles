//! Qualify imported function calls with their shortest safe module path.

use super::Rule;
use super::common::FnCallSuggestion;
use super::common::ImportEdits;
use super::common::LocalNames;
use super::common::calls_in_scope;
use super::common::expected_fn_path;
use super::common::local_names;
use super::common::module_idx;
use super::common::module_idx::ModuleIdx;
use super::common::module_idx::ModuleScope;
use super::common::module_idx::ScopeInfo;
use super::common::module_idx::has_external_modules;
use super::common::path_resolution::path_label;
use super::common::remove_unused_fn_imports;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct UnqualifiedCallRule;

impl Rule for UnqualifiedCallRule {
    fn id(&self) -> &'static str {
        "unqualified-call"
    }

    fn fix(&self, ctx: &FileContext<'_>, editor: &mut SourceEditor<'_>) -> rootcause::Result<()> {
        let idx = module_idx(ctx.file);

        for scope in &idx.scopes {
            fix_scope(ctx, &idx, scope, editor)?;
        }

        Ok(())
    }
}

fn fix_scope(
    ctx: &FileContext<'_>,
    idx: &ModuleIdx<'_>,
    scope: &ModuleScope<'_>,
    editor: &mut SourceEditor<'_>,
) -> rootcause::Result<()> {
    let Some(info) = idx.info.get(&scope.path) else {
        return Ok(());
    };

    let mut imports = ImportEdits::new(scope, info);
    let reserved = local_names(scope.items);
    let external_references = has_external_modules(scope.items);
    let mut changed = Vec::new();

    for call in calls_in_scope(scope.items) {
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

        if !editor.can_replace_path(span) {
            continue;
        }

        let source_path = ctx.source.get(editor.span_range(span)).unwrap_or_default();

        if let Some(reason) = skip_reason(source_path, &suggestion, info, &reserved, external_references) {
            let start = span.start();
            editor.skip(format!(
                "line {}:{}: cannot rewrite `{source_path}` as `{}`: {reason}",
                start.line,
                start.column.saturating_add(1),
                suggestion.expected_path,
            ));
            continue;
        }

        if let Some(import) = &suggestion.required_import
            && !imports.request(import, editor)
        {
            continue;
        }

        editor.replace_span(span, &suggestion.expected_path)?;
        changed.push(span);
    }

    remove_unused_fn_imports(ctx, scope.items, &changed, editor)?;

    imports.apply(ctx, scope, editor)
}

fn skip_reason(
    source_path: &str,
    suggestion: &FnCallSuggestion,
    info: &ScopeInfo,
    reserved: &LocalNames,
    external_references: bool,
) -> Option<&'static str> {
    let module_name = suggestion.expected_path.split("::").next().unwrap_or_default();

    if external_references {
        Some("a child module in another file may still use the function import")
    } else if info.unknown_imports {
        Some("a glob, attributed, or absolute import makes this module's bindings unknown")
    } else if suggestion.requires_crate_context(source_path) {
        Some("resolving the crate or parent module requires information outside this file")
    } else if reserved.has_unknown_bindings() {
        Some("a macro or unresolved block import may introduce conflicting bindings in this module")
    } else if reserved.contains(module_name) {
        Some("a local binding in this module may shadow the target module name")
    } else if reserved.contains(source_path) {
        Some("a local binding elsewhere in this module has the same name as this function")
    } else {
        None
    }
}
