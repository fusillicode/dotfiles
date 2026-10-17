//! Remove private import aliases only when every reference can be rewritten in this file.

use std::collections::HashSet;

use proc_macro2::Span;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use syn::Attribute;
use syn::Expr;
use syn::FieldValue;
use syn::Ident;
use syn::Item;
use syn::ItemMod;
use syn::ItemUse;
use syn::Macro;
use syn::Meta;
use syn::Path;
use syn::UseRename;
use syn::UseTree;
use syn::Visibility;
use syn::visit::Visit;

use super::Rule;
use super::common::local_names;
use super::common::module_idx;
use super::common::module_idx::ModuleIdx;
use super::common::module_idx::ModuleScope;
use super::common::module_idx::has_external_modules;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct AliasedImportRule;

impl Rule for AliasedImportRule {
    fn id(&self) -> &'static str {
        "aliased-import"
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

    let mut claimed: HashSet<_> = info
        .definitions
        .iter()
        .cloned()
        .chain(info.imports.iter().map(|binding| binding.name.clone()))
        .collect();
    let local = local_names(scope.items);
    let mut aliases = Vec::new();

    for item in scope.items {
        if let Item::Use(import) = item {
            collect_aliases(import, &import.tree, None, &mut aliases);
        }
    }

    for ImportAlias {
        import,
        rename,
        original,
    } in aliases
    {
        let alias = rename.rename.to_string();

        if alias == "_" {
            continue;
        }

        let mut references = AliasReferences::new(&alias, &original);

        for item in scope.items {
            references.visit_item(item);
        }

        let cross_module = idx.scopes.iter().filter(|other| other.path != scope.path).any(|other| {
            let mut references = AliasReferences::new(&alias, &original);

            for item in other.items {
                references.visit_item(item);
            }

            !references.replacements.is_empty() || references.opaque
        });

        let clash =
            alias != original && (claimed.contains(&original) || local.contains(&original) || local.contains(&alias));
        let unknown = info.unknown_imports || cross_module || references.opaque || has_external_modules(scope.items);
        let exported = !matches!(import.vis, Visibility::Inherited);

        if clash || unknown || !import.attrs.is_empty() || (exported && alias != original) {
            editor.skip(format!(
                "cannot remove alias {alias}: clashes, exported references, or unresolved syntax"
            ));
            continue;
        }

        let suffix = editor.offset(rename.ident.span().end())..editor.offset(rename.rename.span().end());

        if ctx
            .source
            .get(suffix.clone())
            .is_some_and(|text| text.contains("/*") || text.contains("//"))
        {
            editor.skip("alias contains comments");
            continue;
        }

        editor.replace(suffix, String::new())?;

        for (span, replacement) in references.replacements {
            editor.replace_span(span, replacement)?;
        }

        claimed.insert(original);
    }

    Ok(())
}

struct ImportAlias<'ast> {
    import: &'ast ItemUse,
    rename: &'ast UseRename,
    original: String,
}

fn collect_aliases<'ast>(
    import: &'ast ItemUse,
    tree: &'ast UseTree,
    parent: Option<&'ast Ident>,
    aliases: &mut Vec<ImportAlias<'ast>>,
) {
    let mut pending = vec![(tree, parent)];

    while let Some((tree, parent)) = pending.pop() {
        match tree {
            UseTree::Path(path) => pending.push((&path.tree, Some(&path.ident))),
            UseTree::Group(group) => {
                pending.extend(group.items.iter().rev().map(|tree| (tree, parent)));
            }
            UseTree::Rename(rename) => {
                let original = if rename.ident == "self" {
                    parent.map(ToString::to_string)
                } else {
                    Some(rename.ident.to_string())
                };

                if let Some(original) = original {
                    aliases.push(ImportAlias {
                        import,
                        rename,
                        original,
                    });
                }
            }
            UseTree::Name(_) | UseTree::Glob(_) => {}
        }
    }
}

struct AliasReferences<'name> {
    alias: &'name str,
    original: &'name str,
    replacements: Vec<(Span, String)>,
    opaque: bool,
}

impl<'name> AliasReferences<'name> {
    const fn new(alias: &'name str, original: &'name str) -> Self {
        Self {
            alias,
            original,
            replacements: Vec::new(),
            opaque: false,
        }
    }

    fn tokens(&mut self, tokens: TokenStream) {
        let mut pending = vec![tokens.into_iter()];

        while let Some(tokens) = pending.last_mut() {
            match tokens.next() {
                Some(TokenTree::Ident(ident)) if ident == self.alias => self.opaque = true,
                Some(TokenTree::Group(group)) => pending.push(group.stream().into_iter()),
                Some(TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_)) => {}
                None => {
                    pending.pop();
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for AliasReferences<'_> {
    fn visit_path(&mut self, path: &'ast Path) {
        if let Some(first) = path.segments.first()
            && first.ident == self.alias
            && path.leading_colon.is_none()
        {
            self.replacements.push((first.ident.span(), self.original.to_owned()));
        }

        if path.segments.iter().skip(1).any(|segment| segment.ident == self.alias) {
            self.opaque = true;
        }

        syn::visit::visit_path(self, path);
    }

    fn visit_field_value(&mut self, field: &'ast FieldValue) {
        if field.colon_token.is_none()
            && let Expr::Path(path) = &field.expr
            && path.path.is_ident(self.alias)
        {
            self.replacements.push((
                path.path
                    .segments
                    .first()
                    .map_or_else(Span::call_site, |segment| segment.ident.span()),
                format!("{}: {}", self.alias, self.original),
            ));
            return;
        }

        syn::visit::visit_field_value(self, field);
    }

    fn visit_item_mod(&mut self, _module: &'ast ItemMod) {}

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        let bindings = super::common::import_resolution::use_bindings(&item.tree, &[]);

        if bindings
            .bindings
            .iter()
            .any(|binding| binding.source_path.iter().any(|part| part == self.alias))
        {
            self.opaque = true;
        }
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        self.tokens(mac.tokens.clone());
        self.visit_path(&mac.path);
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.visit_path(attribute.path());

        if let Meta::List(meta) = &attribute.meta {
            self.tokens(meta.tokens.clone());
        }
    }

    fn visit_expr(&mut self, expression: &'ast Expr) {
        if let Expr::Verbatim(tokens) = expression {
            self.tokens(tokens.clone());
        }

        syn::visit::visit_expr(self, expression);
    }

    fn visit_item(&mut self, item: &'ast Item) {
        if let Item::Verbatim(tokens) = item {
            self.tokens(tokens.clone());
        }

        syn::visit::visit_item(self, item);
    }
}
