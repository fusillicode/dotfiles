//! Shared analysis for `frs rsl` rules.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use proc_macro2::Span;
use syn::Expr;
use syn::Item;
use syn::PathArguments;
use syn::UseTree;
use syn::spanned::Spanned;
use syn::visit::Visit;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) struct CallDetails {
    pub(super) actual_path: String,
    pub(super) replacement_path: String,
    pub(super) add_import: Option<String>,
}

pub(super) struct ModuleScope<'ast> {
    pub(super) path: Vec<String>,
    pub(super) items: &'ast [Item],
}

#[derive(Default)]
pub(super) struct ScopeInfo {
    pub(super) definitions: HashSet<String>,
    pub(super) functions: HashSet<String>,
    pub(super) glob_imports: Vec<Vec<String>>,
    pub(super) imports: Vec<ImportBinding>,
    pub(super) unknown_imports: bool,
}

pub(super) struct ImportBinding {
    pub(super) name: String,
    pub(super) path: Vec<String>,
    pub(super) source_path: Vec<String>,
}

pub(super) struct FunctionCallSuggestion {
    pub(super) expected_path: String,
    pub(super) required_import: Option<String>,
}

pub(super) struct ModuleIndex<'ast> {
    pub(super) scopes: Vec<ModuleScope<'ast>>,
    pub(super) info: HashMap<Vec<String>, ScopeInfo>,
    pub(super) modules: HashSet<Vec<String>>,
}

pub(super) fn module_index(file: &syn::File) -> ModuleIndex<'_> {
    let mut index = ModuleIndex {
        scopes: Vec::new(),
        info: HashMap::new(),
        modules: HashSet::new(),
    };
    let mut pending = VecDeque::from([(Vec::new(), file.items.as_slice())]);

    while let Some((path, items)) = pending.pop_front() {
        let mut info = ScopeInfo::default();
        for item in items {
            if let Some(name) = self::item_name(item) {
                info.definitions.insert(name);
            }
            if let Item::Fn(function) = item {
                info.functions.insert(function.sig.ident.to_string());
            }
            if let Item::Mod(module) = item {
                let mut nested_path = path.clone();
                nested_path.push(module.ident.to_string());
                index.modules.insert(nested_path.clone());
                if let Some((_, nested_items)) = &module.content {
                    pending.push_back((nested_path, nested_items.as_slice()));
                }
            }
            if let Item::Use(item_use) = item {
                let bindings = self::use_bindings(&item_use.tree, &path);
                info.imports.extend(bindings.bindings);
                info.glob_imports.extend(bindings.glob_imports);
                info.unknown_imports |= bindings.unknown;
            }
        }
        index.info.insert(path.clone(), info);
        index.scopes.push(ModuleScope { path, items });
    }

    index
}

fn item_name(item: &Item) -> Option<String> {
    match item {
        Item::Const(item) => Some(item.ident.to_string()),
        Item::Enum(item) => Some(item.ident.to_string()),
        Item::ExternCrate(item) => Some(item.ident.to_string()),
        Item::Fn(item) => Some(item.sig.ident.to_string()),
        Item::Mod(item) => Some(item.ident.to_string()),
        Item::Static(item) => Some(item.ident.to_string()),
        Item::Struct(item) => Some(item.ident.to_string()),
        Item::Trait(item) => Some(item.ident.to_string()),
        Item::TraitAlias(item) => Some(item.ident.to_string()),
        Item::Type(item) => Some(item.ident.to_string()),
        Item::Union(item) => Some(item.ident.to_string()),
        Item::ForeignMod(_) | Item::Impl(_) | Item::Macro(_) | Item::Use(_) | Item::Verbatim(_) | _ => None,
    }
}

#[derive(Default)]
struct UseBindings {
    bindings: Vec<ImportBinding>,
    glob_imports: Vec<Vec<String>>,
    unknown: bool,
}

fn use_bindings(tree: &UseTree, current_module: &[String]) -> UseBindings {
    let mut bindings = UseBindings::default();
    let mut pending = vec![(tree, Vec::<String>::new())];

    while let Some((tree, prefix)) = pending.pop() {
        match tree {
            UseTree::Path(path) => {
                let mut next_prefix = prefix;
                next_prefix.push(path.ident.to_string());
                pending.push((path.tree.as_ref(), next_prefix));
            }
            UseTree::Group(group) => {
                pending.extend(group.items.iter().map(|tree| (tree, prefix.clone())));
            }
            UseTree::Name(name) => {
                if name.ident == "self" {
                    if let Some(binding) = prefix.last() {
                        bindings.bindings.push(ImportBinding {
                            name: binding.clone(),
                            path: self::normalize_path(current_module, &prefix),
                            source_path: prefix,
                        });
                    }
                } else {
                    let mut path = prefix;
                    path.push(name.ident.to_string());
                    bindings.bindings.push(ImportBinding {
                        name: name.ident.to_string(),
                        path: self::normalize_path(current_module, &path),
                        source_path: path,
                    });
                }
            }
            UseTree::Rename(rename) => {
                if rename.rename != "_" {
                    let mut path = prefix;
                    path.push(rename.ident.to_string());
                    bindings.bindings.push(ImportBinding {
                        name: rename.rename.to_string(),
                        path: self::normalize_path(current_module, &path),
                        source_path: path,
                    });
                }
            }
            UseTree::Glob(_) => {
                bindings
                    .glob_imports
                    .push(self::normalize_path(current_module, &prefix));
                bindings.unknown = true;
            }
        }
    }

    bindings
}

pub(super) fn path_parts(path: &syn::Path) -> Option<Vec<String>> {
    if path.leading_colon.is_some() || path.segments.is_empty() {
        return None;
    }

    let last_segment = path.segments.len().saturating_sub(1);
    let mut parts = Vec::new();
    for (index, segment) in path.segments.iter().enumerate() {
        if index != last_segment && !matches!(&segment.arguments, PathArguments::None) {
            return None;
        }
        parts.push(segment.ident.to_string());
    }
    Some(parts)
}

pub(super) fn has_name_clash_parts(index: &ModuleIndex<'_>, current_module: &[String], parts: &[String]) -> bool {
    let Some(scope) = index.info.get(current_module) else {
        return true;
    };
    if scope.unknown_imports {
        return true;
    }

    let Some(name) = parts.last() else {
        return true;
    };
    let Some(target_module_parts) = parts.get(..parts.len().saturating_sub(1)) else {
        return true;
    };
    let target_module = self::normalize_path(current_module, target_module_parts);
    let target_path = self::normalize_path(current_module, parts);

    if target_module != current_module && scope.definitions.contains(name) {
        return true;
    }

    scope
        .imports
        .iter()
        .any(|binding| binding.name == *name && binding.path != target_path)
}

pub(super) fn imported_binding<'index>(
    index: &'index ModuleIndex<'_>,
    current_module: &[String],
    name: &str,
) -> Option<&'index ImportBinding> {
    let scope = index.info.get(current_module)?;
    if let Some(binding) = self::direct_imported_binding(scope, name) {
        return Some(binding);
    }

    let mut candidate = None;
    for glob_module in &scope.glob_imports {
        let Some(glob_scope) = index.info.get(glob_module) else {
            // TODO: Resolve explicit imports exported by modules outside this file.
            continue;
        };
        let Some(binding) = self::direct_imported_binding(glob_scope, name) else {
            continue;
        };
        if candidate.is_some() {
            // TODO: Resolve ambiguous explicit imports exported by multiple glob sources.
            return None;
        }
        candidate = Some(binding);
    }
    candidate
}

pub(super) fn associated_receiver_parts(path: &syn::Path) -> Option<Vec<String>> {
    let parts = path_parts(path)?;
    is_associated_function_path(&parts)
        .then(|| parts.get(..parts.len().saturating_sub(1)))
        .flatten()
        .map(ToOwned::to_owned)
}

pub(super) fn is_associated_function_path(parts: &[String]) -> bool {
    parts
        .get(..parts.len().saturating_sub(1))
        .is_some_and(|prefix| prefix.iter().any(|part| is_type_name(part)))
}

pub(super) fn is_non_function_call_path(parts: &[String]) -> bool {
    parts.last().is_some_and(|name| is_type_name(name))
}

pub(super) fn is_import_style_path(parts: &[String]) -> bool {
    parts
        .first()
        .is_some_and(|part| matches!(part.as_str(), "crate" | "self" | "super") || is_module_name(part))
}

fn is_module_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

fn is_type_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn direct_imported_binding<'index>(scope: &'index ScopeInfo, name: &str) -> Option<&'index ImportBinding> {
    let mut bindings = scope.imports.iter().filter(|binding| binding.name == name);
    let binding = bindings.next()?;
    bindings.next().is_none().then_some(binding)
}

pub(super) fn normalize_path(current_module: &[String], parts: &[String]) -> Vec<String> {
    let mut absolute_parts = current_module.to_vec();
    let mut first_item = 0;

    match parts.first().map(String::as_str) {
        Some("crate") => {
            absolute_parts.clear();
            first_item = 1;
        }
        Some("self") => first_item = 1,
        Some("super") => {
            while parts.get(first_item).is_some_and(|part| part == "super") {
                if absolute_parts.pop().is_none() {
                    return Vec::new();
                }
                first_item = first_item.saturating_add(1);
            }
        }
        Some(_) | None => {}
    }

    if let Some(suffix) = parts.get(first_item..) {
        absolute_parts.extend(suffix.iter().cloned());
    }
    absolute_parts
}

pub(super) fn path_label(path: &syn::Path) -> String {
    self::path_parts(path).map_or_else(|| "qualified path".to_owned(), |parts| parts.join("::"))
}

#[derive(Clone, Copy)]
pub(super) enum FunctionCallKind {
    Unqualified,
    Overqualified,
}

pub(super) struct FunctionCallFinding {
    pub(super) span: Span,
    pub(super) actual_path: String,
    pub(super) suggestion: FunctionCallSuggestion,
}

pub(super) fn find_function_calls(file: &syn::File, kind: FunctionCallKind) -> Vec<FunctionCallFinding> {
    let index = self::module_index(file);
    let mut findings = Vec::new();

    for scope in &index.scopes {
        let mut visitor = FunctionCallVisitor {
            index: &index,
            current_module: &scope.path,
            kind,
            findings: &mut findings,
            local_bindings: Vec::new(),
        };
        for item in scope.items {
            visitor.visit_item(item);
        }
    }

    findings
}

struct FunctionCallVisitor<'index, 'ast, 'output> {
    index: &'index ModuleIndex<'ast>,
    current_module: &'index [String],
    kind: FunctionCallKind,
    findings: &'output mut Vec<FunctionCallFinding>,
    local_bindings: Vec<HashSet<String>>,
}

impl<'ast> Visit<'ast> for FunctionCallVisitor<'_, '_, '_> {
    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && let Some(parts) = path_parts(&path.path)
            && let Some(suggestion) =
                expected_function_path(self.index, self.current_module, &self.local_bindings, &path.path)
        {
            let is_relevant = match self.kind {
                FunctionCallKind::Unqualified => parts.len() == 1,
                FunctionCallKind::Overqualified => parts.len() > 1,
            };
            let actual_path = path_label(&path.path);
            if is_relevant && actual_path != suggestion.expected_path {
                self.findings.push(FunctionCallFinding {
                    span: path.span(),
                    actual_path,
                    suggestion,
                });
            }
        }

        syn::visit::visit_expr_call(self, expression);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.local_bindings.push(block_function_bindings(block));
        syn::visit::visit_block(self, block);
        let _ = self.local_bindings.pop();
    }

    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        self.local_bindings.push(closure_bindings(&closure.inputs));
        syn::visit::visit_expr_closure(self, closure);
        let _ = self.local_bindings.pop();
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast syn::ExprForLoop) {
        for attribute in &expression.attrs {
            self.visit_attribute(attribute);
        }
        if let Some(label) = &expression.label {
            self.visit_label(label);
        }
        self.visit_pat(&expression.pat);
        self.visit_expr(&expression.expr);

        self.local_bindings.push(pattern_bindings(&expression.pat));
        self.visit_block(&expression.body);
        let _ = self.local_bindings.pop();
    }

    fn visit_expr_if(&mut self, expression: &'ast syn::ExprIf) {
        if let Expr::Let(let_expression) = expression.cond.as_ref() {
            for attribute in &expression.attrs {
                self.visit_attribute(attribute);
            }
            self.visit_expr(&expression.cond);
            self.local_bindings.push(pattern_bindings(&let_expression.pat));
            self.visit_block(&expression.then_branch);
            let _ = self.local_bindings.pop();
            if let Some((_, else_branch)) = &expression.else_branch {
                self.visit_expr(else_branch);
            }
            return;
        }

        // TODO: Carry bindings through let chains in compound conditions.
        syn::visit::visit_expr_if(self, expression);
    }

    fn visit_expr_while(&mut self, expression: &'ast syn::ExprWhile) {
        if let Expr::Let(let_expression) = expression.cond.as_ref() {
            for attribute in &expression.attrs {
                self.visit_attribute(attribute);
            }
            if let Some(label) = &expression.label {
                self.visit_label(label);
            }
            self.visit_expr(&expression.cond);
            self.local_bindings.push(pattern_bindings(&let_expression.pat));
            self.visit_block(&expression.body);
            let _ = self.local_bindings.pop();
            return;
        }

        // TODO: Carry bindings through let chains in compound conditions.
        syn::visit::visit_expr_while(self, expression);
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        self.local_bindings.push(parameter_bindings(&function.sig.inputs));
        syn::visit::visit_impl_item_fn(self, function);
        let _ = self.local_bindings.pop();
    }

    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        self.local_bindings.push(parameter_bindings(&function.sig.inputs));
        syn::visit::visit_item_fn(self, function);
        let _ = self.local_bindings.pop();
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        for attribute in &local.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_pat(&local.pat);
        if let Some(init) = &local.init {
            self.visit_expr(&init.expr);
            if let Some((_, diverge)) = &init.diverge {
                self.visit_expr(diverge);
            }
        }
        if let Some(scope) = self.local_bindings.last_mut() {
            add_pattern_bindings(scope, &local.pat);
        }
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        self.local_bindings.push(pattern_bindings(&arm.pat));
        syn::visit::visit_arm(self, arm);
        let _ = self.local_bindings.pop();
    }

    fn visit_trait_item_fn(&mut self, function: &'ast syn::TraitItemFn) {
        self.local_bindings.push(parameter_bindings(&function.sig.inputs));
        syn::visit::visit_trait_item_fn(self, function);
        let _ = self.local_bindings.pop();
    }

    fn visit_item_mod(&mut self, _module: &'ast syn::ItemMod) {}

    fn visit_attribute(&mut self, _attribute: &'ast syn::Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast syn::Macro) {
        // TODO: Resolve local bindings generated by macros.
    }
}

struct BindingCollector<'bindings> {
    bindings: &'bindings mut HashSet<String>,
}

impl<'ast> Visit<'ast> for BindingCollector<'_> {
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.bindings.insert(pattern.ident.to_string());
        syn::visit::visit_pat_ident(self, pattern);
    }
}

fn add_pattern_bindings(bindings: &mut HashSet<String>, pattern: &syn::Pat) {
    let mut collector = BindingCollector { bindings };
    collector.visit_pat(pattern);
}

fn pattern_bindings(pattern: &syn::Pat) -> HashSet<String> {
    let mut bindings = HashSet::new();
    add_pattern_bindings(&mut bindings, pattern);
    bindings
}

fn closure_bindings(inputs: &syn::punctuated::Punctuated<syn::Pat, syn::token::Comma>) -> HashSet<String> {
    let mut bindings = HashSet::new();
    for input in inputs {
        add_pattern_bindings(&mut bindings, input);
    }
    bindings
}

fn parameter_bindings(inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>) -> HashSet<String> {
    let mut bindings = HashSet::new();
    for input in inputs {
        match input {
            syn::FnArg::Receiver(_) => {
                bindings.insert("self".to_owned());
            }
            syn::FnArg::Typed(input) => add_pattern_bindings(&mut bindings, &input.pat),
        }
    }
    bindings
}

fn block_function_bindings(block: &syn::Block) -> HashSet<String> {
    block
        .stmts
        .iter()
        .filter_map(|statement| match statement {
            syn::Stmt::Item(Item::Fn(function)) => Some(function.sig.ident.to_string()),
            syn::Stmt::Expr(_, _) | syn::Stmt::Item(_) | syn::Stmt::Local(_) | syn::Stmt::Macro(_) => None,
        })
        .collect()
}

pub(super) fn expected_function_path(
    index: &ModuleIndex<'_>,
    current_module: &[String],
    local_bindings: &[HashSet<String>],
    path: &syn::Path,
) -> Option<FunctionCallSuggestion> {
    let parts = self::path_parts(path)?;
    if self::is_non_function_call_path(&parts) || self::is_associated_function_path(&parts) {
        return None;
    }

    if parts.len() == 1 && parts.first().is_some_and(|name| is_local_binding(local_bindings, name)) {
        return None;
    }

    if local_function(index, current_module, path).is_some() && function_path_tail(&parts).len() <= 2 {
        return None;
    }

    let name = parts.last()?;
    if parts.len() == 1 {
        // TODO: Resolve bare calls whose definitions are outside the indexed file.
        return imported_function_path(index, current_module, name);
    }

    let qualified = function_path_tail(&parts);
    if qualified.len() <= 2 || !can_use_shortened_module(index, current_module, &parts) {
        return None;
    }

    let expected_path = qualified
        .get(qualified.len().saturating_sub(2)..)
        .map(|parts| parts.join("::"))?;
    let function_index = parts.len().saturating_sub(1);
    let target_module = self::normalize_path(current_module, parts.get(..function_index).unwrap_or_default());

    Some(FunctionCallSuggestion {
        expected_path,
        required_import: required_module_import(index, current_module, &parts, &target_module),
    })
}

fn is_local_binding(local_bindings: &[HashSet<String>], name: &str) -> bool {
    local_bindings.iter().rev().any(|bindings| bindings.contains(name))
}

fn imported_function_path(
    index: &ModuleIndex<'_>,
    current_module: &[String],
    name: &str,
) -> Option<FunctionCallSuggestion> {
    let binding = self::imported_binding(index, current_module, name)?;
    let source_path = &binding.path;
    if !can_use_imported_module(index, current_module, source_path) {
        return None;
    }

    let expected_path = (source_path.len() >= 2)
        .then(|| source_path.get(source_path.len().saturating_sub(2)..))
        .flatten()
        .map(|parts| parts.join("::"))?;
    let function_index = source_path.len().saturating_sub(1);
    let target_module = source_path.get(..function_index)?;

    Some(FunctionCallSuggestion {
        expected_path,
        required_import: required_module_import(index, current_module, &binding.source_path, target_module),
    })
}

fn required_module_import(
    index: &ModuleIndex<'_>,
    current_module: &[String],
    source_path: &[String],
    target_module: &[String],
) -> Option<String> {
    let module_name = target_module.last()?;
    if module_is_available(index, current_module, module_name, target_module) {
        return None;
    }

    let source_module_path = source_path.get(..source_path.len().saturating_sub(1))?;
    let import_path = match source_module_path.first().map(String::as_str) {
        Some("crate" | "self" | "super") => crate_path(target_module),
        Some(_) if source_module_path.len() == 1 && !index.modules.contains(target_module) => return None,
        Some(_) if index.modules.contains(target_module) => crate_path(target_module),
        Some(_) => source_module_path.join("::"),
        None => return None,
    };

    Some(format!("use {import_path};"))
}

fn module_is_available(
    index: &ModuleIndex<'_>,
    current_module: &[String],
    module_name: &str,
    target_module: &[String],
) -> bool {
    let Some(scope) = index.info.get(current_module) else {
        return false;
    };
    if scope
        .imports
        .iter()
        .any(|binding| binding.name == module_name && binding.path == target_module)
    {
        return true;
    }

    let mut local_module = current_module.to_vec();
    local_module.push(module_name.to_owned());
    scope.definitions.contains(module_name) && local_module == target_module && index.modules.contains(&local_module)
}

fn crate_path(parts: &[String]) -> String {
    std::iter::once("crate".to_owned())
        .chain(parts.iter().cloned())
        .collect::<Vec<_>>()
        .join("::")
}

fn can_use_shortened_module(index: &ModuleIndex<'_>, current_module: &[String], parts: &[String]) -> bool {
    let Some(function_index) = parts.len().checked_sub(1) else {
        return false;
    };
    let Some(module_name) = parts.get(function_index.saturating_sub(1)) else {
        return false;
    };
    let target_module = self::normalize_path(current_module, parts.get(..function_index).unwrap_or_default());

    can_use_module_name(index, current_module, module_name, &target_module)
}

fn can_use_imported_module(index: &ModuleIndex<'_>, current_module: &[String], source_path: &[String]) -> bool {
    let Some(function_index) = source_path.len().checked_sub(1) else {
        return false;
    };
    let Some(module_name) = source_path.get(function_index.saturating_sub(1)) else {
        return false;
    };
    let target_module = source_path.get(..function_index).unwrap_or_default();

    // The explicit function import resolves the callable; defer only the module-name conflict to future glob
    // resolution.
    if index
        .info
        .get(current_module)
        .is_some_and(|scope| scope.unknown_imports)
    {
        return true;
    }

    can_use_module_name(index, current_module, module_name, target_module)
}

fn can_use_module_name(
    index: &ModuleIndex<'_>,
    current_module: &[String],
    name: &str,
    target_module: &[String],
) -> bool {
    let Some(scope) = index.info.get(current_module) else {
        return false;
    };
    // TODO: Resolve glob imports before deciding whether the shortened module name conflicts.
    if scope.unknown_imports {
        return false;
    }

    let mut local_module = current_module.to_vec();
    local_module.push(name.to_owned());
    if scope.definitions.contains(name) && (!index.modules.contains(&local_module) || local_module != target_module) {
        return false;
    }

    scope
        .imports
        .iter()
        .filter(|binding| binding.name == name)
        .all(|binding| binding.path == target_module)
}

fn function_path_tail(parts: &[String]) -> &[String] {
    let mut first = 0;
    while matches!(parts.get(first).map(String::as_str), Some("crate" | "self" | "super")) {
        first = first.saturating_add(1);
    }
    parts.get(first..).unwrap_or_default()
}

fn local_function(
    index: &ModuleIndex<'_>,
    current_module: &[String],
    path: &syn::Path,
) -> Option<(Vec<String>, String)> {
    let parts = self::path_parts(path)?;
    let name = parts.last()?.clone();
    let last_item = parts.len().saturating_sub(1);
    let mut module_path = current_module.to_vec();
    let mut first_item = 0;

    match parts.first()?.as_str() {
        "crate" => {
            module_path.clear();
            first_item = 1;
        }
        "self" => first_item = 1,
        "super" => {
            while parts.get(first_item).is_some_and(|part| part == "super") {
                module_path.pop()?;
                first_item = first_item.saturating_add(1);
            }
        }
        _ => {}
    }

    if first_item > last_item {
        return None;
    }
    module_path.extend(parts.get(first_item..last_item)?.iter().cloned());
    if !is_local_module_path(index, &module_path) {
        return None;
    }

    let scope = index.info.get(&module_path)?;
    if scope.imports.iter().any(|binding| binding.name == name) {
        return None;
    }
    if scope.functions.contains(&name) {
        return Some((module_path, name));
    }
    if let Some(glob_module) = glob_imported_function(index, scope, &name) {
        return Some((glob_module, name));
    }
    if scope.unknown_imports {
        return None;
    }
    None
}

fn glob_imported_function(index: &ModuleIndex<'_>, scope: &self::ScopeInfo, name: &str) -> Option<Vec<String>> {
    let mut candidate = None;
    for glob_module in &scope.glob_imports {
        let Some(glob_scope) = index.info.get(glob_module) else {
            // TODO: Resolve glob imports from modules outside this file.
            continue;
        };
        if !glob_scope.functions.contains(name) {
            continue;
        }
        if candidate.is_some() {
            // TODO: Resolve ambiguous names imported from multiple glob sources.
            return None;
        }
        candidate = Some(glob_module.clone());
    }
    candidate
}

fn is_local_module_path(index: &ModuleIndex<'_>, path: &[String]) -> bool {
    path.is_empty() || index.modules.contains(path)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::super::aliased_import::AliasedImportRule;
    use super::super::aliased_import::AliasedImportViolation;
    use super::super::common::CallDetails;
    use super::super::overqualified_call::OverqualifiedCallRule;
    use super::super::overqualified_call::OverqualifiedCallViolation;
    use super::super::qualified_item::QualifiedItemDetails;
    use super::super::qualified_item::QualifiedItemRule;
    use super::super::qualified_item::QualifiedItemViolation;
    use super::super::unqualified_call::UnqualifiedCallRule;
    use super::super::unqualified_call::UnqualifiedCallViolation;
    use crate::cmds::rsl::rules::TypedRule;

    #[test]
    fn test_unqualified_call_check_when_same_module_call_is_bare_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}
            fn run() {
                helper();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_same_module_call_is_bare_with_glob_import_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod tests {
                use super::*;

                fn helper() {}

                fn run() {
                    helper();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_parent_function_is_bare_with_super_glob_import_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}

            mod tests {
                use super::*;
                use test_that::prelude::*;

                fn invoke() {
                    helper();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_explicit_import_is_mixed_with_glob_import_reports_imported_module() {
        let syntax = syn::parse_file(
            r"
            mod external {
                pub fn run() {}
            }
            mod tests {
                use super::*;
                use crate::external::run;

                fn invoke() {
                    run();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![UnqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 10,
                column: 21,
                details: CallDetails {
                    actual_path: "run".to_owned(),
                    replacement_path: "external::run".to_owned(),
                    add_import: Some("use crate::external;".to_owned()),
                },
            }])
        );
    }

    #[test]
    fn test_unqualified_call_check_when_parent_explicit_import_is_reexported_by_glob_reports_imported_module() {
        let syntax = syn::parse_file(
            r"
            mod external {
                pub fn run() {}
            }
            use crate::external::run;

            mod tests {
                use super::*;

                fn invoke() {
                    run();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![UnqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 11,
                column: 21,
                details: CallDetails {
                    actual_path: "run".to_owned(),
                    replacement_path: "external::run".to_owned(),
                    add_import: Some("use crate::external;".to_owned()),
                },
            }])
        );
    }

    #[test]
    fn test_unqualified_call_check_when_glob_imported_call_is_unresolved_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod external {
                pub fn imported() {}
            }
            mod tests {
                use super::*;

                fn run() {
                    imported();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_same_module_call_uses_self_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}
            fn run() {
                self::helper();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_nested_function_call_is_local_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn run() {
                fn helper() {}
                helper();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_callable_parameter_is_called_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn run(check: impl Fn()) {
                check();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_closure_binding_is_called_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn run() {
                let check = || {};
                check();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_local_binding_shadows_imported_function_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod external {
                pub fn check() {}
            }
            use external::check;
            fn run() {
                let check = || {};
                check();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_imported_foreign_call_is_bare_reports_call() {
        let syntax = syn::parse_file(
            r"
            mod external;
            use external::run;
            fn main() {
                run();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![UnqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 5,
                column: 17,
                details: CallDetails {
                    actual_path: "run".to_owned(),
                    replacement_path: "external::run".to_owned(),
                    add_import: None,
                },
            }])
        );
    }

    #[test]
    fn test_unqualified_call_check_when_imported_external_crate_call_is_bare_omits_import() {
        let syntax = syn::parse_file(
            r"
            use tempfile::tempdir;
            fn main() {
                tempdir();
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![UnqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 4,
                column: 17,
                details: CallDetails {
                    actual_path: "tempdir".to_owned(),
                    replacement_path: "tempfile::tempdir".to_owned(),
                    add_import: None,
                },
            }])
        );
    }

    #[test]
    fn test_overqualified_call_check_when_foreign_module_call_has_one_module_prefix_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod helper {
                pub fn run() {}
            }
            fn main() {
                helper::run();
            }
            ",
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_foreign_module_call_uses_crate_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod helper {
                pub fn run() {}
            }
            fn main() {
                crate::helper::run();
            }
            ",
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_free_function_call_has_one_module_prefix_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            fn read() {
                fs::read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_free_function_call_has_multiple_module_prefixes_reports_call() {
        let syntax = syn::parse_file(
            r#"
            fn read() {
                std::fs::read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![OverqualifiedCallViolation {
                file: PathBuf::from("test.rs"),
                line: 3,
                column: 17,
                details: CallDetails {
                    actual_path: "std::fs::read_to_string".to_owned(),
                    replacement_path: "fs::read_to_string".to_owned(),
                    add_import: Some("use std::fs;".to_owned()),
                },
            }])
        );
    }

    #[test]
    fn test_overqualified_call_check_when_shortened_module_name_conflicts_with_import_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            mod other {
                pub mod fs {}
            }
            use crate::other::fs;
            fn read() {
                std::fs::read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_shortened_module_name_conflicts_with_local_module_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            mod fs {}
            fn read() {
                std::fs::read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_overqualified_call_check_when_imported_function_module_name_conflicts_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod source {
                pub fn run() {}
            }
            mod other {
                pub mod source {}
            }
            use crate::other::source;
            use crate::source::run;
            fn main() {
                run();
            }
            ",
        )
        .unwrap();

        let result = OverqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_associated_function_receiver_is_uppercase_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            fn open() {
                File::open("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_bare_uppercase_constructor_is_called_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn read() {
                Ok(());
                Err(());
                Some(1);
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_bare_prelude_function_is_called_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn read(value: usize) {
                drop(value);
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_unknown_bare_function_call_returns_no_violations() {
        let syntax = syn::parse_file(
            r#"
            fn read() {
                read_to_string("foo.md");
            }
            "#,
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_qualified_item_check_when_non_function_path_is_fully_qualified_reports_import() {
        let syntax = syn::parse_file(
            r"
            mod values {
                pub const VALUE: usize = 1;
            }
            fn read() -> usize {
                crate::values::VALUE
            }
            ",
        )
        .unwrap();

        let result = QualifiedItemRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualifiedItemViolation {
                file: PathBuf::from("test.rs"),
                line: 6,
                column: 17,
                details: QualifiedItemDetails {
                    actual_path: "crate::values::VALUE".to_owned(),
                    expected_import: "use crate::values::VALUE;".to_owned(),
                },
            }])
        );
    }

    #[test]
    fn test_qualified_item_check_when_non_function_name_clashes_allows_qualified_path() {
        let syntax = syn::parse_file(
            r"
            mod values {
                pub const VALUE: usize = 1;
            }
            const VALUE: usize = 2;
            fn read() -> usize {
                crate::values::VALUE
            }
            ",
        )
        .unwrap();

        let result = QualifiedItemRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_qualified_item_check_when_struct_names_clash_allows_one_qualified_path() {
        let syntax = syn::parse_file(
            r"
            mod first {
                pub struct Thing;
            }
            mod second {
                pub struct Thing;
            }
            use crate::first::Thing;
            fn read() -> crate::second::Thing {
                panic!()
            }
            ",
        )
        .unwrap();

        let result = QualifiedItemRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_aliased_import_check_when_alias_is_private_reports_alias() {
        let syntax = syn::parse_file(
            r"
            use std::fmt::Display as Formatter;
            ",
        )
        .unwrap();

        let result = AliasedImportRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![AliasedImportViolation {
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 38,
            }])
        );
    }

    #[test]
    fn test_aliased_import_check_when_alias_is_wildcard_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            use std::fmt::Display as _;
            ",
        )
        .unwrap();

        let result = AliasedImportRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_aliased_import_check_when_public_reexport_is_renamed_reports_alias() {
        let syntax = syn::parse_file(
            r"
            pub use std::fmt::Debug as Formatter;
            ",
        )
        .unwrap();

        let result = AliasedImportRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![AliasedImportViolation {
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 40,
            }])
        );
    }

    #[test]
    fn test_aliased_import_check_when_reexport_alias_is_restricted_reports_alias() {
        let syntax = syn::parse_file(
            r"
            pub(crate) use std::fmt::Debug as Formatter;
            ",
        )
        .unwrap();

        let result = AliasedImportRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![AliasedImportViolation {
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 47,
            }])
        );
    }

    #[test]
    fn test_qualified_item_check_when_external_paths_are_qualified_reports_paths() {
        let syntax = syn::parse_file(
            r"
            mod external;
            use external::Thing;
            struct Data;
            impl Data {
                fn run() {}
                fn call(&self) {
                    self.run();
                    Self::run();
                }
            }
            fn read() -> external::Thing {
                external::Thing::new()
            }
            ",
        )
        .unwrap();

        let result = QualifiedItemRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![
                QualifiedItemViolation {
                    file: PathBuf::from("test.rs"),
                    line: 12,
                    column: 26,
                    details: QualifiedItemDetails {
                        actual_path: "external::Thing".to_owned(),
                        expected_import: "use external::Thing;".to_owned(),
                    },
                },
                QualifiedItemViolation {
                    file: PathBuf::from("test.rs"),
                    line: 13,
                    column: 17,
                    details: QualifiedItemDetails {
                        actual_path: "external::Thing".to_owned(),
                        expected_import: "use external::Thing;".to_owned(),
                    },
                },
            ])
        );
    }

    #[test]
    fn test_qualified_item_check_when_unknown_external_type_is_qualified_reports_import() {
        let syntax = syn::parse_file(
            r"
            fn inspect(_: syn::ExprCall) {}
            ",
        )
        .unwrap();

        let result = QualifiedItemRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualifiedItemViolation {
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 27,
                details: QualifiedItemDetails {
                    actual_path: "syn::ExprCall".to_owned(),
                    expected_import: "use syn::ExprCall;".to_owned(),
                },
            }])
        );
    }

    #[test]
    fn test_qualified_item_check_when_enum_variant_is_qualified_ignores_path() {
        let syntax = syn::parse_file(
            r"
            enum Kind {
                First,
                Second,
            }
            fn select(kind: Kind) -> Kind {
                match kind {
                    Kind::First => Kind::Second,
                    Kind::Second => Kind::First,
                }
            }
            ",
        )
        .unwrap();

        let result = QualifiedItemRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_qualified_item_check_when_associated_function_is_referenced_ignores_path() {
        let syntax = syn::parse_file(
            r"
            fn converter() -> fn(&String) -> &str {
                String::as_str
            }
            ",
        )
        .unwrap();

        let result = QualifiedItemRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_unqualified_call_check_when_nested_module_call_is_bare_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod outer {
                fn helper() {}
                fn run() {
                    helper();
                }
            }
            ",
        )
        .unwrap();

        let result = UnqualifiedCallRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }
    #[test]
    fn test_unqualified_call_violation_formats_compact_output() {
        let violation = UnqualifiedCallViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 13,
            details: CallDetails {
                actual_path: "run()".to_owned(),
                replacement_path: "self::run()".to_owned(),
                add_import: None,
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:2:13,uc,replace `run()` with `self::run()`"
        );
    }

    #[test]
    fn test_overqualified_call_violation_formats_compact_output() {
        let violation = OverqualifiedCallViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 13,
            details: CallDetails {
                actual_path: "run()".to_owned(),
                replacement_path: "external::run()".to_owned(),
                add_import: Some("use crate::external;".to_owned()),
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:2:13,oc,replace `run()` with `external::run()`; add `use crate::external;`"
        );
    }

    #[test]
    fn test_qualified_item_violation_formats_compact_output() {
        let violation = QualifiedItemViolation {
            file: PathBuf::from("test.rs"),
            line: 3,
            column: 13,
            details: QualifiedItemDetails {
                actual_path: "external::Thing".to_owned(),
                expected_import: "use external::Thing;".to_owned(),
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:3:13,qualified_item,replace `external::Thing` with `Thing`; add `use external::Thing;`"
        );
    }

    #[test]
    fn test_aliased_import_violation_formats_compact_output() {
        let violation = AliasedImportViolation {
            file: PathBuf::from("test.rs"),
            line: 4,
            column: 13,
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:4:13,aliased_import,use unaliased import if there are no clashes"
        );
    }
}
