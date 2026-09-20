use std::collections::HashSet;

use proc_macro2::Span;
use syn::Expr;
use syn::spanned::Spanned;
use syn::visit::Visit;

use super::fn_path_resolution::FnCallSuggestion;
use super::fn_path_resolution::expected_fn_path;
use super::module_idx::ModuleIdx;
use super::module_idx::module_idx;
use super::path_resolution::path_label;
use super::path_resolution::path_parts;
use super::scope_bindings::add_pattern_bindings;
use super::scope_bindings::block_fn_bindings;
use super::scope_bindings::closure_bindings;
use super::scope_bindings::parameter_bindings;
use super::scope_bindings::pattern_bindings;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct CallDetails {
    pub actual_path: String,
    pub replacement_path: String,
    pub add_import: Option<String>,
}

#[derive(Clone, Copy)]
pub enum FnCallKind {
    Unqualified,
    Overqualified,
}

pub struct FnCallFinding {
    pub span: Span,
    pub actual_path: String,
    pub suggestion: FnCallSuggestion,
}

pub fn find_fn_calls(file: &syn::File, kind: FnCallKind) -> Vec<FnCallFinding> {
    let idx = module_idx(file);
    let mut findings = Vec::new();

    for scope in &idx.scopes {
        let mut visitor = FnCallVisitor {
            idx: &idx,
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

struct FnCallVisitor<'idx, 'ast, 'output> {
    idx: &'idx ModuleIdx<'ast>,
    current_module: &'idx [String],
    kind: FnCallKind,
    findings: &'output mut Vec<FnCallFinding>,
    local_bindings: Vec<HashSet<String>>,
}

impl<'ast> Visit<'ast> for FnCallVisitor<'_, '_, '_> {
    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && let Some(parts) = path_parts(&path.path)
            && let Some(suggestion) = expected_fn_path(self.idx, self.current_module, &self.local_bindings, &path.path)
        {
            let is_relevant = match self.kind {
                FnCallKind::Unqualified => parts.len() == 1,
                FnCallKind::Overqualified => parts.len() > 1,
            };
            let actual_path = path_label(&path.path);
            if is_relevant && actual_path != suggestion.expected_path {
                self.findings.push(FnCallFinding {
                    span: path.span(),
                    actual_path,
                    suggestion,
                });
            }
        }

        syn::visit::visit_expr_call(self, expression);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.local_bindings.push(block_fn_bindings(block));
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

    fn visit_impl_item_fn(&mut self, fn_item: &'ast syn::ImplItemFn) {
        self.local_bindings.push(parameter_bindings(&fn_item.sig.inputs));
        syn::visit::visit_impl_item_fn(self, fn_item);
        let _ = self.local_bindings.pop();
    }

    fn visit_item_fn(&mut self, fn_item: &'ast syn::ItemFn) {
        self.local_bindings.push(parameter_bindings(&fn_item.sig.inputs));
        syn::visit::visit_item_fn(self, fn_item);
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

    fn visit_trait_item_fn(&mut self, fn_item: &'ast syn::TraitItemFn) {
        self.local_bindings.push(parameter_bindings(&fn_item.sig.inputs));
        syn::visit::visit_trait_item_fn(self, fn_item);
        let _ = self.local_bindings.pop();
    }

    fn visit_item_mod(&mut self, _module: &'ast syn::ItemMod) {}

    fn visit_attribute(&mut self, _attribute: &'ast syn::Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast syn::Macro) {
        // TODO: Resolve local bindings generated by macros.
    }
}
