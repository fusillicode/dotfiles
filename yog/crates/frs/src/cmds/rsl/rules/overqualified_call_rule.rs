//! Shorten qualified function calls to their shortest safe module path.

use super::Rule;
use super::common::ImportEdits;
use super::common::calls_in_scope;
use super::common::expected_fn_path;
use super::common::local_names;
use super::common::module_idx;
use super::common::module_idx::ModuleIdx;
use super::common::module_idx::ModuleScope;
use super::common::path_resolution::path_label;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct OverqualifiedCallRule;

impl Rule for OverqualifiedCallRule {
    fn id(&self) -> &'static str {
        "overqualified-call"
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

        if !editor.can_replace_path(span) {
            continue;
        }

        let source_path = ctx.source.get(editor.span_range(span)).unwrap_or_default();
        let module_name = suggestion.expected_path.split("::").next().unwrap_or_default();

        if info.unknown_imports || suggestion.requires_crate_context(source_path) || reserved.contains(module_name) {
            editor.skip("cannot resolve an unshadowed module name");
            continue;
        }

        if let Some(import) = &suggestion.required_import
            && !imports.request(import, editor)
        {
            continue;
        }

        editor.replace_span(span, &suggestion.expected_path)?;
    }

    imports.apply(ctx, scope, editor)
}
