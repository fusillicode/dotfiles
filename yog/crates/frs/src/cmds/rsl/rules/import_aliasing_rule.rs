//! Use an imported item's original name instead of giving it another name with `as`.
//!
//! For example, change `use std::path::PathBuf as FilePath;` to `use std::path::PathBuf;`
//! and use `PathBuf` wherever that import was used as `FilePath`.
//!
//! Keep the alias when removing it could cause a name conflict, change a name exposed to other code,
//! or require changes that cannot be checked from this file. Imports named `_` stay unchanged.
//! Also keep aliases mentioned in quoted text inside attributes or macros.

use std::collections::HashSet;
use std::ops::Range;

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
use syn::Lit;
use syn::LitStr;
use syn::Macro;
use syn::Meta;
use syn::Path;
use syn::UseRename;
use syn::UseTree;
use syn::Visibility;
use syn::visit;
use syn::visit::Visit;

use super::Rule;
use super::common;
use super::common::ImportError;
use super::common::import_resolution;
use super::common::module_idx;
use super::common::module_idx::ModuleIdx;
use super::common::module_idx::ModuleScope;
use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct ImportAliasingRule;

impl Rule for ImportAliasingRule {
    fn id(&self) -> &'static str {
        "import-aliasing"
    }

    fn analyze<'source>(&self, ctx: &FileContext<'source>) -> rootcause::Result<RulePlan<'source>> {
        let mut plan = RulePlan {
            edits: SourceEditor::new(ctx.source),
            findings: Vec::new(),
        };
        let idx = common::module_idx(ctx.file);

        for scope in &idx.scopes {
            analyze_scope(&idx, scope, &mut plan)?;
        }

        Ok(plan)
    }
}

struct Change {
    span: Span,
    alias: String,
    original: String,
    alias_suffix: Range<usize>,
    references: Vec<(Span, String)>,
}

impl Change {
    fn add_to_plan(&self, plan: &mut RulePlan<'_>) -> rootcause::Result<()> {
        plan.edits.replace(self.alias_suffix.clone(), String::new())?;
        for (span, replacement) in &self.references {
            plan.edits.replace_span(*span, replacement)?;
        }
        plan.record_fixable(self);
        Ok(())
    }
}

impl RuleChange for Change {
    fn span(&self) -> Span {
        self.span
    }

    fn description(&self) -> String {
        format!("replace alias `{}` with `{}`", self.alias, self.original)
    }
}

enum BlockReason {
    ExportedAlias,
    AttributedImport,
    CrossModuleReferences,
    UnresolvedReferences,
    ExternalModules,
}

impl From<BlockReason> for FixBlockReason {
    fn from(reason: BlockReason) -> Self {
        match reason {
            BlockReason::ExportedAlias => Self {
                code: "exported-alias",
                message: "Other code may depend on the name exported by this import.".to_owned(),
                help: "Keep this exported name unless its callers can also be updated.",
            },
            BlockReason::AttributedImport => Self {
                code: "attributed-import",
                message: "Conditional attributes may change whether this import is available.".to_owned(),
                help: "Review the import in each supported configuration before changing it manually.",
            },
            BlockReason::CrossModuleReferences => Self {
                code: "cross-module-references",
                message: "Other modules in this file may still use this alias.".to_owned(),
                help: "Review references in the other modules before changing this name.",
            },
            BlockReason::UnresolvedReferences => Self {
                code: "unresolved-references",
                message: "References inside macros, attributes, or other unresolved syntax cannot be updated safely."
                    .to_owned(),
                help: "Review the affected names and make this change manually if appropriate.",
            },
            BlockReason::ExternalModules => Self {
                code: "external-modules",
                message: "A child module in another file may still use the existing import.".to_owned(),
                help: "Review references in the other modules before changing this name.",
            },
        }
    }
}

fn analyze_scope(idx: &ModuleIdx<'_>, scope: &ModuleScope<'_>, plan: &mut RulePlan<'_>) -> rootcause::Result<()> {
    let Some(info) = idx.info.get(&scope.path) else {
        return Ok(());
    };

    let mut claimed: HashSet<_> = info
        .definitions
        .iter()
        .cloned()
        .chain(info.imports.iter().map(|binding| binding.name.clone()))
        .collect();
    let local = common::local_names(scope.items);
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

        let mut references = AliasReferences {
            alias: &alias,
            original: &original,
            replacements: Vec::new(),
            opaque: false,
        };

        for item in scope.items {
            references.visit_item(item);
        }

        let cross_module = idx.scopes.iter().filter(|other| other.path != scope.path).any(|other| {
            let mut references = AliasReferences {
                alias: &alias,
                original: &original,
                replacements: Vec::new(),
                opaque: false,
            };

            for item in other.items {
                references.visit_item(item);
            }

            !references.replacements.is_empty() || references.opaque
        });

        let clash =
            alias != original && (claimed.contains(&original) || local.contains(&original) || local.contains(&alias));
        let exported = !matches!(import.vis, Visibility::Inherited);
        let reason = if exported && alias != original {
            Some(FixBlockReason::from(BlockReason::ExportedAlias))
        } else if common::has_conditional_attributes(&import.attrs) {
            Some(BlockReason::AttributedImport.into())
        } else if info.unknown_imports {
            Some(ImportError::UnknownImports.into())
        } else if alias != original && local.has_unknown_bindings() {
            Some(ImportError::UnknownBindings.into())
        } else if clash {
            Some(ImportError::NameConflict { name: original.clone() }.into())
        } else if cross_module {
            Some(BlockReason::CrossModuleReferences.into())
        } else if references.opaque {
            Some(BlockReason::UnresolvedReferences.into())
        } else if module_idx::has_external_modules(scope.items) {
            Some(BlockReason::ExternalModules.into())
        } else {
            None
        };

        let replacements = references.replacements;
        let change = Change {
            span: rename.rename.span(),
            alias,
            original,
            alias_suffix: plan.edits.offset(rename.ident.span().end())..plan.edits.offset(rename.rename.span().end()),
            references: replacements,
        };

        if let Some(reason) = reason {
            plan.record_blocked(&change, reason);
            continue;
        }

        let text = plan.edits.source_range(change.alias_suffix.clone());
        if text.contains("/*") || text.contains("//") {
            plan.record_blocked(&change, common::comments_reason());
            continue;
        }

        change.add_to_plan(plan)?;
        claimed.insert(change.original);
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

impl AliasReferences<'_> {
    fn tokens(&mut self, tokens: TokenStream) {
        let mut pending = vec![tokens];

        while let Some(tokens) = pending.pop() {
            for token in tokens {
                match token {
                    TokenTree::Ident(name) => self.opaque |= name == self.alias,
                    TokenTree::Group(group) => pending.push(group.stream()),
                    TokenTree::Literal(literal) => {
                        if let Ok(string) = syn::parse2::<LitStr>(TokenTree::Literal(literal).into()) {
                            self.string_reference(&string);
                        }
                    }
                    TokenTree::Punct(_) => {}
                }
            }
        }
    }

    fn string_reference(&mut self, string: &LitStr) {
        // Macros can interpret quoted text as Rust paths. Without knowing their
        // grammar, keep aliases mentioned in that text rather than rewriting it.
        // Match conservatively, including within longer text, without assuming Rust syntax.
        self.opaque |= string.value().contains(self.alias);
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

        visit::visit_path(self, path);
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

        visit::visit_field_value(self, field);
    }

    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        for attribute in &module.attrs {
            self.visit_attribute(attribute);
        }
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        for attribute in &item.attrs {
            self.visit_attribute(attribute);
        }

        let bindings = import_resolution::use_bindings(&item.tree, &[]);

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

        match &attribute.meta {
            Meta::List(meta) => self.tokens(meta.tokens.clone()),
            Meta::NameValue(meta) => {
                if let Expr::Lit(expression) = &meta.value
                    && let Lit::Str(string) = &expression.lit
                {
                    self.string_reference(string);
                }

                self.visit_expr(&meta.value);
            }
            Meta::Path(_) => {}
        }
    }

    fn visit_expr(&mut self, expression: &'ast Expr) {
        if let Expr::Verbatim(tokens) = expression {
            self.tokens(tokens.clone());
        }

        visit::visit_expr(self, expression);
    }

    fn visit_item(&mut self, item: &'ast Item) {
        if let Item::Verbatim(tokens) = item {
            self.tokens(tokens.clone());
        }

        visit::visit_item(self, item);
    }
}
