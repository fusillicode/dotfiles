//! Import qualified items and replace only their module prefix, preserving generic arguments.
//! Keep listed type names qualified, regardless of their path; leave lowercase function paths alone.

use proc_macro2::Span;
use syn::Attribute;
use syn::ItemMod;
use syn::ItemUse;
use syn::Macro;
use syn::Path;
use syn::visit::Visit;

use super::Rule;
use super::common::ImportEdits;
use super::common::has_name_clash_parts;
use super::common::is_import_style_path;
use super::common::local_names;
use super::common::module_idx;
use super::common::module_idx::ModuleIdx;
use super::common::module_idx::ModuleScope;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

const KEEP_QUALIFIED_NAMES: &[&str] = &[
    "AsMut",
    "AsRef",
    "AsyncFn",
    "AsyncFnMut",
    "AsyncFnOnce",
    "Box",
    "Clone",
    "Copy",
    "Default",
    "DoubleEndedIterator",
    "Drop",
    "Eq",
    "Err",
    "Error",
    "ExactSizeIterator",
    "Extend",
    "Fn",
    "FnMut",
    "FnOnce",
    "From",
    "FromIterator",
    "Future",
    "Into",
    "IntoFuture",
    "IntoIterator",
    "Iterator",
    "None",
    "Ok",
    "Option",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "Result",
    "Send",
    "Sized",
    "Some",
    "String",
    "Sync",
    "ToOwned",
    "ToString",
    "TryFrom",
    "TryInto",
    "Unpin",
    "Vec",
];

pub(super) struct QualifiedItemRule;

impl Rule for QualifiedItemRule {
    fn id(&self) -> &'static str {
        "qualified-item"
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

    let mut visitor = QualifiedPaths { paths: Vec::new() };

    for item in scope.items {
        visitor.visit_item(item);
    }

    let mut imports = ImportEdits::new(scope, info);
    let local = local_names(scope.items);

    for (span, parts) in visitor.paths {
        let path = parts.join("::");
        let Some(name) = parts.last() else {
            continue;
        };

        if KEEP_QUALIFIED_NAMES.contains(&name.as_str()) {
            continue;
        }

        if !editor.can_replace_path(span) {
            continue;
        }

        if has_name_clash_parts(idx, &scope.path, &parts) {
            editor.skip(format!("cannot import {path}: name may clash"));
            continue;
        }

        if matches!(parts.first().map(String::as_str), Some("crate" | "super")) {
            editor.skip("cannot resolve this path from the supplied file alone");
            continue;
        }

        let same_module = super::common::path_resolution::normalize_path(&scope.path, &parts)
            == scope
                .path
                .iter()
                .cloned()
                .chain(std::iter::once(name.clone()))
                .collect::<Vec<_>>();

        if same_module && local.contains(name) {
            editor.skip(format!("cannot shorten {path}: {name} is shadowed"));
            continue;
        }

        if !(same_module && info.definitions.contains(name) || imports.request(&path, editor)) {
            continue;
        }

        editor.replace_span(span, name)?;
    }

    imports.apply(ctx, scope, editor)
}

struct QualifiedPaths {
    paths: Vec<(Span, Vec<String>)>,
}

impl QualifiedPaths {
    fn collect(&mut self, path: &Path) {
        if path.leading_colon.is_some() {
            return;
        }

        let segments: Vec<_> = path.segments.iter().collect();
        let Some(type_position) = segments
            .iter()
            .position(|segment| segment.ident.to_string().chars().next().is_some_and(char::is_uppercase))
        else {
            return;
        };

        let end = type_position.saturating_add(1);
        let prefix = segments.get(..end).unwrap_or_default();

        if prefix.len() < 2 {
            return;
        }

        let parts: Vec<_> = prefix.iter().map(|segment| segment.ident.to_string()).collect();

        if !is_import_style_path(&parts) {
            return;
        }

        if let (Some(first), Some(last)) = (prefix.first(), prefix.last())
            && let Some(span) = first.ident.span().join(last.ident.span())
        {
            self.paths.push((span, parts));
        }
    }
}

impl<'ast> Visit<'ast> for QualifiedPaths {
    fn visit_path(&mut self, path: &'ast Path) {
        self.collect(path);
        syn::visit::visit_path(self, path);
    }

    fn visit_item_mod(&mut self, _module: &'ast ItemMod) {}

    fn visit_item_use(&mut self, _item: &'ast ItemUse) {}

    fn visit_attribute(&mut self, _attribute: &'ast Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast Macro) {}
}
