//! Resolve call-site bindings in their lexical scopes and Rust namespaces.

use std::collections::HashMap;
use std::collections::HashSet;
use std::mem;

use proc_macro2::Span;
use proc_macro2::TokenStream;
use syn::Arm;
use syn::Attribute;
use syn::BinOp;
use syn::Block;
use syn::Expr;
use syn::ExprCall;
use syn::ExprClosure;
use syn::ExprForLoop;
use syn::ExprIf;
use syn::ExprPath;
use syn::ExprStruct;
use syn::ExprWhile;
use syn::Fields;
use syn::FnArg;
use syn::ForeignItem;
use syn::GenericParam;
use syn::Generics;
use syn::ImplItemFn;
use syn::Item;
use syn::ItemFn;
use syn::ItemImpl;
use syn::ItemMod;
use syn::ItemTrait;
use syn::ItemUse;
use syn::Local;
use syn::Macro;
use syn::Meta;
use syn::Pat;
use syn::PatGuard;
use syn::PatIdent;
use syn::PatStruct;
use syn::Path;
use syn::Stmt;
use syn::TraitItemFn;
use syn::TypePath;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::visit;
use syn::visit::Visit;

use super::fn_path_resolution::CallTarget;
use super::fn_path_resolution::FnCallSuggestion;
use super::fn_path_resolution::ModuleAccess;
use super::import_edits::ImportError;
use super::import_resolution::use_bindings;
use super::module_idx::ModuleIdx;
use super::module_idx::ModuleScope;
use super::module_idx::ScopeInfo;

pub fn analyze_calls<'ast>(scope: &ModuleScope<'ast>, idx: &ModuleIdx<'ast>) -> CallAnalysis<'ast> {
    let items = scope.items;
    let module_bindings = Bindings::items(items, &scope.path);
    let mut visitor = FnCallVisitor {
        idx,
        calls: Vec::new(),
        scopes: Vec::new(),
        declarations: Bindings::declarations(items),
        module_bindings,
        current_module: scope.path.clone(),
        references: References::default(),
        free_type_names: HashSet::new(),
        path_namespace: Namespace::Type,
        conditional: false,
    };
    for item in items {
        visitor.visit_item(item);
    }
    CallAnalysis {
        calls: visitor.calls,
        references: visitor.references,
        free_type_names: visitor.free_type_names,
    }
}

pub struct CallAnalysis<'ast> {
    pub calls: Vec<CallSite<'ast>>,
    pub references: References,
    pub free_type_names: HashSet<String>,
}

/// Proven references in this module stay separate from names in opaque syntax or children.
#[derive(Default)]
pub struct References {
    pub values: HashMap<String, usize>,
    pub types: HashMap<String, usize>,
    pub tokens: HashSet<String>,
    pub uncertain: HashSet<String>,
}

pub struct CallSite<'ast> {
    pub path: &'ast Path,
    pub conditional: bool,
    scopes: Vec<Bindings>,
    module_value: bool,
}

impl CallSite<'_> {
    pub fn is_bare(&self) -> bool {
        self.path.segments.len() == 1
    }

    pub fn is_locally_bound(&self) -> bool {
        self.module_value
            || self.path.segments.first().is_some_and(|segment| {
                let name = segment.ident.to_string();
                self.scopes
                    .iter()
                    .any(|scope| scope.values.contains(&name) || scope.locals.contains(&name))
            })
    }

    pub fn resolve_target(&self, suggestion: &FnCallSuggestion, info: &ScopeInfo) -> Result<CallTarget, ImportError> {
        self.check_source()?;

        let CallTarget::Module { path, access } = &suggestion.target else {
            let name = &suggestion.expected_path;
            if self
                .scopes
                .iter()
                .any(|scope| scope.values.contains(name) || scope.locals.contains(name))
            {
                return Err(ImportError::NameConflict { name: name.clone() });
            }

            return Ok(CallTarget::SameModule);
        };

        let name = suggestion.expected_path.split("::").next().unwrap_or_default();
        if let Some(binding) = self.scopes.iter().rev().find_map(|scope| scope.types.get(name)) {
            return match binding {
                TypeBinding::Import(imported) if imported == path => Ok(CallTarget::Module {
                    path: path.clone(),
                    access: ModuleAccess::Local,
                }),
                TypeBinding::Declaration | TypeBinding::Import(_) => {
                    Err(ImportError::NameConflict { name: name.to_owned() })
                }
            };
        }
        if info.unknown_imports && !matches!(access, ModuleAccess::Imported(_) | ModuleAccess::Declared) {
            return Err(ImportError::UnknownImports);
        }
        Ok(suggestion.target.clone())
    }

    pub fn span(&self) -> Span {
        self.path
            .segments
            .first()
            .and_then(|first| {
                self.path
                    .segments
                    .last()
                    .and_then(|last| first.ident.span().join(last.ident.span()))
            })
            .unwrap_or_else(|| self.path.span())
    }

    fn check_module_name(&self, name: &str) -> Result<(), ImportError> {
        if self.scopes.iter().any(|scope| scope.unknown) {
            return Err(ImportError::UnknownBindings);
        }
        if self.scopes.iter().any(|scope| scope.types.contains_key(name)) {
            return Err(ImportError::NameConflict { name: name.to_owned() });
        }
        Ok(())
    }

    fn check_source(&self) -> Result<(), ImportError> {
        // A qualified source may itself be shadowed by a block import or type.
        // The module-only path resolver must not reinterpret that spelling.
        let name = if self.is_bare() {
            String::new()
        } else {
            self.path
                .segments
                .first()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default()
        };
        self.check_module_name(&name)
    }
}

pub(super) fn module_type_names(items: &[Item]) -> HashSet<String> {
    Bindings::declarations(items).types.into_keys().collect()
}

struct ConditionalAttributes {
    found: bool,
}

impl<'ast> Visit<'ast> for ConditionalAttributes {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        // Keep a containing item's imports stable across all configurations,
        // including cfg attributes on statements, arms, and expressions.
        self.found |= attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr");
    }
}

/// Module qualifiers use the type namespace; callable locals use the value namespace.
/// Imports are reserved in every namespace until their definitions are known.
/// Macro names are separate, so a local value named println does not shadow println!.
#[derive(Clone, Default)]
struct Bindings {
    types: HashMap<String, TypeBinding>,
    values: HashSet<String>,
    locals: HashSet<String>,
    unknown: bool,
}

impl Bindings {
    fn declarations(items: &[Item]) -> Self {
        let mut bindings = Self::default();
        for item in items {
            if !matches!(item, Item::Use(import) if !super::has_conditional_attributes(&import.attrs)) {
                bindings.add_item(item, &[]);
            }
        }
        bindings
    }

    fn items(items: &[Item], current_module: &[String]) -> Self {
        let mut bindings = Self::default();
        for item in items {
            bindings.add_item(item, current_module);
        }
        bindings
    }

    fn add_item(&mut self, item: &Item, current_module: &[String]) {
        if let Item::ExternCrate(item) = item {
            let name = item.rename.as_ref().map_or(&item.ident, |(_, name)| name);
            self.types.insert(name.to_string(), TypeBinding::Declaration);
            return;
        }
        if let Item::ForeignMod(module) = item {
            for item in &module.items {
                if let ForeignItem::Type(item) = item {
                    self.types.insert(item.ident.to_string(), TypeBinding::Declaration);
                } else if let ForeignItem::Fn(item) = item {
                    self.values.insert(item.sig.ident.to_string());
                } else if let ForeignItem::Static(item) = item {
                    self.values.insert(item.ident.to_string());
                }
            }
            return;
        }
        if let Some(name) = super::module_idx::item_name(item) {
            if matches!(item, Item::Fn(_) | Item::Const(_) | Item::Static(_)) {
                self.values.insert(name);
                return;
            }
            self.types.insert(name.clone(), TypeBinding::Declaration);
            if let Item::Struct(structure) = item
                && !matches!(structure.fields, Fields::Named(_))
            {
                self.values.insert(name);
            }
        } else {
            self.add_import(item, current_module);
        }
    }

    fn add_import(&mut self, item: &Item, current_module: &[String]) {
        if let Item::Use(import) = item {
            let imported = use_bindings(&import.tree, current_module);
            self.unknown |= imported.unknown;
            for binding in imported.bindings {
                let kind = if super::has_conditional_attributes(&import.attrs) {
                    TypeBinding::Declaration
                } else {
                    TypeBinding::Import(binding.path)
                };
                self.types.insert(binding.name.clone(), kind);
                self.values.insert(binding.name.clone());
            }
        }
    }

    fn parameters(inputs: &Punctuated<FnArg, Comma>, generics: &Generics) -> Self {
        let mut bindings = Self::generics(generics);
        for input in inputs {
            match input {
                FnArg::Receiver(_) => {
                    bindings.locals.insert("self".to_owned());
                }
                FnArg::Typed(input) => add_pattern_bindings(&mut bindings.locals, &input.pat),
            }
        }
        bindings
    }

    fn generics(generics: &Generics) -> Self {
        let mut bindings = Self::default();
        for parameter in &generics.params {
            match parameter {
                GenericParam::Type(parameter) => {
                    bindings
                        .types
                        .insert(parameter.ident.to_string(), TypeBinding::Declaration);
                }
                GenericParam::Const(parameter) => {
                    bindings.locals.insert(parameter.ident.to_string());
                }
                GenericParam::Lifetime(_) => {}
            }
        }
        bindings
    }
}

#[derive(Clone)]
enum TypeBinding {
    Declaration,
    Import(Vec<String>),
}

#[derive(Clone, Copy)]
enum Namespace {
    Type,
    Value,
}

struct FnCallVisitor<'ast, 'index> {
    idx: &'index ModuleIdx<'ast>,
    calls: Vec<CallSite<'ast>>,
    scopes: Vec<Bindings>,
    module_bindings: Bindings,
    declarations: Bindings,
    current_module: Vec<String>,
    references: References,
    free_type_names: HashSet<String>,
    path_namespace: Namespace,
    conditional: bool,
}

impl<'ast> FnCallVisitor<'ast, '_> {
    fn record_reference(&mut self, name: String) {
        if self.conditional || self.scopes.iter().any(|scope| scope.unknown) {
            self.references.uncertain.insert(name);
            return;
        }
        let references = match self.path_namespace {
            Namespace::Value => &mut self.references.values,
            Namespace::Type => &mut self.references.types,
        };
        let count = references.entry(name).or_default();
        *count = count.saturating_add(1);
    }

    fn path_is_locally_bound(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .chain([&self.declarations])
            .any(|scope| match self.path_namespace {
                Namespace::Type => scope.types.contains_key(name),
                Namespace::Value => scope.values.contains(name) || scope.locals.contains(name),
            })
    }

    fn record_free_type_name(&mut self, name: String) {
        if !self.module_bindings.types.contains_key(&name)
            && !self.scopes.iter().any(|scope| scope.types.contains_key(&name))
        {
            self.free_type_names.insert(name);
        }
    }

    fn record_macro_tokens(&mut self, tokens: TokenStream) {
        for name in super::token_names(tokens) {
            self.references.tokens.insert(name.clone());
            self.record_free_type_name(name);
        }
    }

    fn visit_condition(&mut self, expression: &'ast Expr) {
        if let Expr::Binary(binary) = expression
            && matches!(binary.op, BinOp::And(_))
        {
            self.visit_condition(&binary.left);
            self.visit_condition(&binary.right);
            return;
        }
        if let Expr::Let(expression) = expression {
            self.visit_expr(&expression.expr);
            if let Some(scope) = self.scopes.last_mut() {
                add_pattern_bindings(&mut scope.locals, &expression.pat);
            }
        } else {
            self.visit_expr(expression);
        }
    }
}

impl<'ast> Visit<'ast> for FnCallVisitor<'ast, '_> {
    fn visit_item(&mut self, item: &'ast Item) {
        let mut conditions = ConditionalAttributes { found: false };
        conditions.visit_item(item);
        let outer = self.conditional;
        self.conditional |= conditions.found;
        visit::visit_item(self, item);
        self.conditional = outer;
    }

    fn visit_path(&mut self, path: &'ast Path) {
        if path.leading_colon.is_none()
            && let Some(segment) = path.segments.first()
        {
            let name = segment.ident.to_string();
            if matches!(name.as_str(), "crate" | "self" | "super") {
                for segment in &path.segments {
                    self.references.uncertain.insert(segment.ident.to_string());
                }
            } else if !self.path_is_locally_bound(&name) {
                self.record_reference(name.clone());
            }
            if path.segments.len() > 1 || matches!(self.path_namespace, Namespace::Type) {
                self.record_free_type_name(name);
            }
        }
        visit::visit_path(self, path);
    }

    fn visit_type_path(&mut self, path: &'ast TypePath) {
        let outer = mem::replace(&mut self.path_namespace, Namespace::Type);
        visit::visit_type_path(self, path);
        self.path_namespace = outer;
    }

    fn visit_expr_struct(&mut self, expression: &'ast ExprStruct) {
        let outer = mem::replace(&mut self.path_namespace, Namespace::Type);
        visit::visit_expr_struct(self, expression);
        self.path_namespace = outer;
    }

    fn visit_pat_struct(&mut self, pattern: &'ast PatStruct) {
        let outer = mem::replace(&mut self.path_namespace, Namespace::Type);
        visit::visit_pat_struct(self, pattern);
        self.path_namespace = outer;
    }

    fn visit_item_use(&mut self, import: &'ast ItemUse) {
        for attribute in &import.attrs {
            self.visit_attribute(attribute);
        }

        let imported = use_bindings(&import.tree, &[]);
        // Import paths can themselves be captured, even when the import defines
        // a same-spelled local binding.
        for path in imported
            .bindings
            .iter()
            .map(|binding| &binding.source_path)
            .chain(imported.glob_imports.iter())
        {
            if import.leading_colon.is_none()
                && let Some(root) = path.first()
            {
                self.free_type_names.insert(root.clone());
                let dependency = if root == "self" { path.get(1) } else { Some(root) };
                if let Some(name) = dependency {
                    self.references.uncertain.insert(name.clone());
                }
            }
        }
    }

    fn visit_expr_path(&mut self, expression: &'ast ExprPath) {
        let namespace = if expression.path.segments.len() == 1 && expression.qself.is_none() {
            Namespace::Value
        } else {
            Namespace::Type
        };
        let outer = mem::replace(&mut self.path_namespace, namespace);
        visit::visit_expr_path(self, expression);
        self.path_namespace = outer;
    }

    fn visit_expr_call(&mut self, expression: &'ast ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && path.qself.is_none()
        {
            self.calls.push(CallSite {
                path: &path.path,
                conditional: self.conditional,
                scopes: self.scopes.clone(),
                module_value: path.path.segments.len() == 1
                    && path
                        .path
                        .segments
                        .first()
                        .is_some_and(|segment| self.declarations.values.contains(&segment.ident.to_string())),
            });
        }
        visit::visit_expr_call(self, expression);
    }

    fn visit_block(&mut self, block: &'ast Block) {
        let mut bindings = Bindings::default();
        for statement in &block.stmts {
            if let Stmt::Item(item) = statement {
                bindings.add_item(item, &self.current_module);
            }
        }

        self.scopes.push(bindings);
        visit::visit_block(self, block);
        let _ = self.scopes.pop();
    }

    fn visit_expr_closure(&mut self, closure: &'ast ExprClosure) {
        let mut bindings = Bindings::default();
        for input in &closure.inputs {
            add_pattern_bindings(&mut bindings.locals, input);
        }
        self.scopes.push(bindings);
        visit::visit_expr_closure(self, closure);
        let _ = self.scopes.pop();
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast ExprForLoop) {
        self.visit_expr(&expression.expr);
        self.scopes.push(pattern_bindings(&expression.pat));
        self.visit_block(&expression.body);
        let _ = self.scopes.pop();
    }

    fn visit_expr_if(&mut self, expression: &'ast ExprIf) {
        self.scopes.push(Bindings::default());
        self.visit_condition(&expression.cond);
        self.visit_block(&expression.then_branch);
        let _ = self.scopes.pop();
        if let Some((_, branch)) = &expression.else_branch {
            self.visit_expr(branch);
        }
    }

    fn visit_expr_while(&mut self, expression: &'ast ExprWhile) {
        self.scopes.push(Bindings::default());
        self.visit_condition(&expression.cond);
        self.visit_block(&expression.body);
        let _ = self.scopes.pop();
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        self.scopes.push(Bindings::generics(&item.generics));
        visit::visit_item_impl(self, item);
        let _ = self.scopes.pop();
    }

    fn visit_item_trait(&mut self, item: &'ast ItemTrait) {
        self.scopes.push(Bindings::generics(&item.generics));
        visit::visit_item_trait(self, item);
        let _ = self.scopes.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.scopes
            .push(Bindings::parameters(&item.sig.inputs, &item.sig.generics));
        visit::visit_impl_item_fn(self, item);
        let _ = self.scopes.pop();
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        // An enclosing local name produces a capture error across an item boundary;
        // it does not fall back to a same-named module import.
        self.scopes
            .push(Bindings::parameters(&item.sig.inputs, &item.sig.generics));
        visit::visit_item_fn(self, item);
        let _ = self.scopes.pop();
    }

    fn visit_local(&mut self, local: &'ast Local) {
        if let Some(init) = &local.init {
            self.visit_expr(&init.expr);
            if let Some((_, diverge)) = &init.diverge {
                self.visit_expr(diverge);
            }
        }
        if let Some(scope) = self.scopes.last_mut() {
            add_pattern_bindings(&mut scope.locals, &local.pat);
        }
    }

    fn visit_arm(&mut self, arm: &'ast Arm) {
        self.scopes.push(pattern_bindings(&arm.pat));
        if let Pat::Guard(pattern) = &arm.pat {
            self.visit_pat(&pattern.pat);
            self.visit_condition(&pattern.guard);
        } else {
            self.visit_pat(&arm.pat);
        }
        self.visit_expr(&arm.body);
        let _ = self.scopes.pop();
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        self.scopes
            .push(Bindings::parameters(&item.sig.inputs, &item.sig.generics));
        visit::visit_trait_item_fn(self, item);
        let _ = self.scopes.pop();
    }

    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        for attribute in &module.attrs {
            self.visit_attribute(attribute);
        }

        if let Some((brace, items)) = &module.content {
            let mut path = self.current_module.clone();
            path.push(module.ident.to_string());
            let scope = ModuleScope {
                path,
                items,
                body_start: brace.span.open().end(),
            };
            let analysis = analyze_calls(&scope, self.idx);
            self.free_type_names.extend(analysis.free_type_names);
            self.references.uncertain.extend(analysis.references.values.into_keys());
            self.references.uncertain.extend(analysis.references.types.into_keys());
            self.references.uncertain.extend(analysis.references.tokens);
            self.references.uncertain.extend(analysis.references.uncertain);
            // Child imports may re-export a private name without calling it.
            for item in items {
                if let Item::Use(import) = item {
                    let bindings = use_bindings(&import.tree, &[]);
                    for binding in bindings.bindings {
                        if binding
                            .source_path
                            .first()
                            .is_some_and(|root| matches!(root.as_str(), "crate" | "self" | "super"))
                        {
                            for name in binding.source_path {
                                self.references.uncertain.insert(name);
                            }
                        }
                    }
                }
            }
        }
    }

    // Follow the source-visible macro policy documented in common: record written
    // names without guessing bindings or references generated by expansion.
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        for segment in &attribute.path().segments {
            self.references.tokens.insert(segment.ident.to_string());
        }
        if attribute.path().leading_colon.is_none()
            && let Some(root) = attribute.path().segments.first()
        {
            self.record_free_type_name(root.ident.to_string());
        }
        if let Meta::List(meta) = &attribute.meta {
            self.record_macro_tokens(meta.tokens.clone());
        }
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        if mac.path.leading_colon.is_none()
            && let Some(root) = mac.path.segments.first()
        {
            self.record_free_type_name(root.ident.to_string());
        }
        self.record_macro_tokens(mac.tokens.clone());
    }
}

fn pattern_bindings(pattern: &Pat) -> Bindings {
    let mut bindings = Bindings::default();
    add_pattern_bindings(&mut bindings.locals, pattern);
    bindings
}

fn add_pattern_bindings(bindings: &mut HashSet<String>, pattern: &Pat) {
    let mut collector = BindingCollector { bindings };
    collector.visit_pat(pattern);
}

struct BindingCollector<'bindings> {
    bindings: &'bindings mut HashSet<String>,
}

impl<'ast> Visit<'ast> for BindingCollector<'_> {
    fn visit_pat_guard(&mut self, pattern: &'ast PatGuard) {
        self.visit_pat(&pattern.pat);
    }

    fn visit_pat_ident(&mut self, pattern: &'ast PatIdent) {
        self.bindings.insert(pattern.ident.to_string());
        visit::visit_pat_ident(self, pattern);
    }
}
