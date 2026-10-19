//! Import items such as types so their names are easier to read where they are used.
//!
//! For example, add `use std::path::PathBuf;` and write `PathBuf` instead of `std::path::PathBuf`.
//! The same applies to uses such as `PathBuf::new()`.
//!
//! Keep module names on common names such as `Result`, `Error`, and `Option` to make their meaning clear.
//! Function calls are handled by the call-qualification rule. Leave names unchanged when shortening them could cause
//! a conflict or when this file does not provide enough information to check the change.
//! Keep names qualified when an import could change other calls or depend on a different build configuration.

use std::collections::HashMap;
use std::collections::HashSet;
use std::iter;

use proc_macro2::Span;
use syn::Attribute;
use syn::ExprPath;
use syn::ExprStruct;
use syn::Item;
use syn::ItemImpl;
use syn::ItemMod;
use syn::ItemUse;
use syn::Macro;
use syn::PatStruct;
use syn::PatTupleStruct;
use syn::Path;
use syn::QSelf;
use syn::TraitBound;
use syn::TypePath;
use syn::visit;
use syn::visit::Visit;

use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::Rule;
use crate::cmds::rsl::rules::common;
use crate::cmds::rsl::rules::common::ImportEdits;
use crate::cmds::rsl::rules::common::ImportError;
use crate::cmds::rsl::rules::common::import_resolution;
use crate::cmds::rsl::rules::common::module_idx::ModuleIdx;
use crate::cmds::rsl::rules::common::module_idx::ModuleScope;
use crate::cmds::rsl::rules::common::module_idx::ScopeInfo;
use crate::cmds::rsl::rules::common::path_resolution;
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

pub(super) struct ItemQualificationRule;

impl Rule for ItemQualificationRule {
    fn id(&self) -> &'static str {
        "item-qualification"
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

struct Change<'path> {
    span: Span,
    path: &'path str,
    name: &'path str,
}

impl Change<'_> {
    fn add_to_plan(&self, plan: &mut RulePlan<'_>) -> rootcause::Result<()> {
        plan.edits.replace_span(self.span, self.name)?;
        plan.record_fixable(self);
        Ok(())
    }
}

impl RuleChange for Change<'_> {
    fn span(&self) -> Span {
        self.span
    }

    fn description(&self) -> String {
        format!("replace `{}` with `{}`", self.path, self.name)
    }
}

enum BlockReason {
    MissingModuleContext,
    TraitImport,
    UnknownItemKind,
}

impl From<BlockReason> for FixBlockReason {
    fn from(reason: BlockReason) -> Self {
        match reason {
            BlockReason::MissingModuleContext => Self {
                code: "parent-module-unknown",
                message: "The required crate or parent-module context is outside this file.".to_owned(),
                help: "Use the correct crate:: path after checking the surrounding modules.",
            },
            BlockReason::TraitImport => Self {
                code: "trait-import",
                message: "Importing this trait could change how existing method calls are resolved.".to_owned(),
                help: "Keep the qualified trait path to preserve method lookup in this scope.",
            },
            BlockReason::UnknownItemKind => Self {
                code: "unknown-item-kind",
                message: "This associated path could refer to a trait or a type.".to_owned(),
                help: "Keep the qualified path unless the item is known to be a type or is already imported.",
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

    let mut visitor = ItemPaths::default();

    for item in scope.items {
        visitor.visit_item(item);
    }

    let traits: HashSet<_> = visitor
        .traits
        .iter()
        .map(|parts| resolve_path(idx, scope, parts))
        .collect();
    let types: HashSet<_> = visitor
        .types
        .iter()
        .map(|parts| resolve_path(idx, scope, parts))
        .collect();
    let mut imports = ImportEdits::new(scope, info);
    let mut planned_imports = HashMap::new();
    let local = common::local_names(scope.items);

    // Unconditional uses may provide imports that conditional uses can safely reuse,
    // including when the conditional occurrence appears earlier in the source.
    visitor.paths.sort_by_key(|path| path.conditional);

    for item in visitor.paths {
        let path = item.parts.join("::");
        let Some(name) = item.parts.last() else {
            continue;
        };

        if KEEP_QUALIFIED_NAMES.contains(&name.as_str()) {
            continue;
        }

        let target = resolve_path(idx, scope, &item.parts);
        let is_trait = traits.contains(&target);
        let binding = info
            .imports
            .iter()
            .find(|binding| binding.name == *name && resolve_path(idx, scope, &binding.source_path) == target);
        let binding = if binding.is_none() && is_trait {
            info.imports
                .iter()
                .find(|binding| resolve_path(idx, scope, &binding.source_path) == target)
        } else {
            binding
        };
        let short_name = binding
            .map(|binding| &binding.name)
            .or_else(|| planned_imports.get(&target))
            .unwrap_or(name);
        let change = Change {
            span: item.span,
            path: &path,
            name: short_name,
        };

        if let Err(reason) = item.check_source(idx, scope, info, &plan.edits) {
            plan.record_blocked(&change, reason);
            continue;
        }

        let same_module = item.is_local_definition(scope, info);
        if (same_module || short_name != name) && local.contains(short_name) {
            let reason = if local.has_unknown_bindings() {
                ImportError::UnknownBindings
            } else {
                ImportError::NameConflict {
                    name: short_name.clone(),
                }
            };
            plan.record_blocked(&change, reason);
            continue;
        }

        let needs_import = !same_module && binding.is_none() && !planned_imports.contains_key(&target);
        let reason = if needs_import && item.conditional {
            Some(common::conditional_code_reason())
        } else if needs_import && is_trait {
            Some(BlockReason::TraitImport.into())
        } else if needs_import && item.associated && !types.contains(&target) {
            Some(BlockReason::UnknownItemKind.into())
        } else {
            None
        };

        if let Some(reason) = reason {
            plan.record_blocked(&change, reason);
            continue;
        }

        if needs_import && let Err(reason) = imports.request(&path) {
            plan.record_blocked(&change, reason);
            continue;
        }

        change.add_to_plan(plan)?;

        if needs_import {
            planned_imports.insert(target, name.clone());
        }
    }

    imports.apply(ctx, scope, &mut plan.edits)
}

// Explicit module aliases identify the same item without needing a type checker.
// Re-exports or ambiguous imports do not establish equivalence here.
fn resolve_path(idx: &ModuleIdx<'_>, scope: &ModuleScope<'_>, parts: &[String]) -> Vec<String> {
    if let Some((root, tail)) = parts.split_first()
        && !matches!(root.as_str(), "crate" | "self" | "super")
        && let Some(binding) = import_resolution::imported_binding(idx, &scope.path, root)
    {
        return binding.path.iter().chain(tail).cloned().collect();
    }

    path_resolution::normalize_path(&scope.path, parts)
}

struct ItemPath {
    span: Span,
    parts: Vec<String>,
    conditional: bool,
    associated: bool,
}

impl ItemPath {
    fn is_local_definition(&self, scope: &ModuleScope<'_>, info: &ScopeInfo) -> bool {
        let Some(name) = self.parts.last() else {
            return false;
        };

        path_resolution::normalize_path(&scope.path, &self.parts)
            == scope
                .path
                .iter()
                .cloned()
                .chain(iter::once(name.clone()))
                .collect::<Vec<_>>()
            && info.definitions.contains(name)
    }

    fn check_source(
        &self,
        idx: &ModuleIdx<'_>,
        scope: &ModuleScope<'_>,
        info: &ScopeInfo,
        edits: &SourceEditor<'_>,
    ) -> Result<(), FixBlockReason> {
        common::ensure_no_comments(edits, self.span)?;

        if info.unknown_imports
            || self
                .parts
                .first()
                .is_some_and(|root| import_resolution::has_conditional_import(scope.items, root))
        {
            return Err(ImportError::UnknownImports.into());
        }

        if common::has_name_clash_parts(idx, &scope.path, &self.parts) {
            return Err(ImportError::NameConflict {
                name: self.parts.last().cloned().unwrap_or_default(),
            }
            .into());
        }

        if matches!(self.parts.first().map(String::as_str), Some("crate" | "super")) {
            return Err(BlockReason::MissingModuleContext.into());
        }

        Ok(())
    }
}

#[derive(Default)]
struct ItemPaths {
    paths: Vec<ItemPath>,
    traits: HashSet<Vec<String>>,
    types: HashSet<Vec<String>>,
    conditional: bool,
}

impl ItemPaths {
    fn path_prefix(&self, path: &Path) -> Option<ItemPath> {
        if path.leading_colon.is_some() {
            return None;
        }

        let segments: Vec<_> = path.segments.iter().collect();
        let type_position = segments
            .iter()
            .position(|segment| segment.ident.to_string().chars().next().is_some_and(char::is_uppercase))?;
        let end = type_position.saturating_add(1);
        let prefix = segments.get(..end)?;

        if prefix.len() < 2 {
            return None;
        }

        let parts: Vec<_> = prefix.iter().map(|segment| segment.ident.to_string()).collect();

        if !common::is_import_style_path(&parts) {
            return None;
        }

        let span = prefix.first()?.ident.span().join(prefix.last()?.ident.span())?;
        Some(ItemPath {
            span,
            parts,
            conditional: self.conditional,
            associated: end < segments.len(),
        })
    }

    fn record_trait(&mut self, path: &Path) {
        if let Some(path) = self.path_prefix(path) {
            self.traits.insert(path.parts);
        }
    }

    fn record_type(&mut self, path: &Path) {
        if !self.conditional
            && let Some(path) = self.path_prefix(path)
            && !path.associated
        {
            self.types.insert(path.parts);
        }
    }

    fn record_qualified_trait(&mut self, qself: Option<&QSelf>, path: &Path) {
        if qself.is_some_and(|qself| qself.position > 0) {
            self.record_trait(path);
        }
    }
}

impl<'ast> Visit<'ast> for ItemPaths {
    fn visit_item(&mut self, item: &'ast Item) {
        let outer = self.conditional;
        self.conditional |= common::contains_conditional_code(item);
        visit::visit_item(self, item);
        self.conditional = outer;
    }

    fn visit_path(&mut self, path: &'ast Path) {
        if let Some(path) = self.path_prefix(path) {
            self.paths.push(path);
        }
        visit::visit_path(self, path);
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        if let Some((path, _)) = &item.trait_ {
            self.record_trait(path);
        }
        visit::visit_item_impl(self, item);
    }

    fn visit_trait_bound(&mut self, bound: &'ast TraitBound) {
        self.record_trait(&bound.path);
        visit::visit_trait_bound(self, bound);
    }

    fn visit_type_path(&mut self, path: &'ast TypePath) {
        self.record_qualified_trait(path.qself.as_ref(), &path.path);
        if path.qself.is_none() {
            self.record_type(&path.path);
        }
        visit::visit_type_path(self, path);
    }

    fn visit_expr_path(&mut self, expression: &'ast ExprPath) {
        self.record_qualified_trait(expression.qself.as_ref(), &expression.path);
        visit::visit_expr_path(self, expression);
    }

    fn visit_expr_struct(&mut self, expression: &'ast ExprStruct) {
        self.record_qualified_trait(expression.qself.as_ref(), &expression.path);
        if expression.qself.is_none() {
            self.record_type(&expression.path);
        }
        visit::visit_expr_struct(self, expression);
    }

    fn visit_pat_struct(&mut self, pattern: &'ast PatStruct) {
        self.record_qualified_trait(pattern.qself.as_ref(), &pattern.path);
        if pattern.qself.is_none() {
            self.record_type(&pattern.path);
        }
        visit::visit_pat_struct(self, pattern);
    }

    fn visit_pat_tuple_struct(&mut self, pattern: &'ast PatTupleStruct) {
        self.record_qualified_trait(pattern.qself.as_ref(), &pattern.path);
        if pattern.qself.is_none() {
            self.record_type(&pattern.path);
        }
        visit::visit_pat_tuple_struct(self, pattern);
    }

    fn visit_item_mod(&mut self, _module: &'ast ItemMod) {}

    fn visit_item_use(&mut self, _item: &'ast ItemUse) {}

    fn visit_attribute(&mut self, _attribute: &'ast Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast Macro) {}
}
