//! Collect path call sites and their lexical binding context within one module scope.

use std::collections::HashSet;

use proc_macro2::Span;
use syn::Expr;
use syn::ExprCall;
use syn::ExprLet;
use syn::Item;
use syn::Path;
use syn::spanned::Spanned;
use syn::visit::Visit;

pub fn calls_in_scope(items: &[Item]) -> Vec<CallSite<'_>> {
    let mut visitor = FnCallVisitor {
        calls: Vec::new(),
        local_bindings: Vec::new(),
    };

    for item in items {
        visitor.visit_item(item);
    }

    visitor.calls
}

pub struct CallSite<'ast> {
    pub path: &'ast Path,
    locally_bound: bool,
}

impl CallSite<'_> {
    pub fn is_bare(&self) -> bool {
        self.path.segments.len() == 1
    }

    pub const fn is_locally_bound(&self) -> bool {
        self.locally_bound
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
}

struct FnCallVisitor<'ast> {
    calls: Vec<CallSite<'ast>>,
    local_bindings: Vec<HashSet<String>>,
}

impl<'ast> Visit<'ast> for FnCallVisitor<'ast> {
    fn visit_expr_call(&mut self, expression: &'ast ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref() {
            let locally_bound = path.path.segments.first().is_some_and(|segment| {
                let name = segment.ident.to_string();
                self.local_bindings
                    .iter()
                    .rev()
                    .any(|bindings| bindings.contains(&name))
            });
            self.calls.push(CallSite {
                path: &path.path,
                locally_bound,
            });
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

        let bindings = condition_bindings(&expression.cond);
        self.local_bindings.push(bindings);
        syn::visit::visit_expr_if(self, expression);
        let _ = self.local_bindings.pop();
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

        let bindings = condition_bindings(&expression.cond);
        self.local_bindings.push(bindings);
        syn::visit::visit_expr_while(self, expression);
        let _ = self.local_bindings.pop();
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

fn block_fn_bindings(block: &syn::Block) -> HashSet<String> {
    block
        .stmts
        .iter()
        .filter_map(|statement| match statement {
            syn::Stmt::Item(Item::Fn(fn_item)) => Some(fn_item.sig.ident.to_string()),
            syn::Stmt::Expr(_, _) | syn::Stmt::Item(_) | syn::Stmt::Local(_) | syn::Stmt::Macro(_) => None,
        })
        .collect()
}

fn condition_bindings(expression: &Expr) -> HashSet<String> {
    let mut visitor = ConditionBindings(HashSet::new());
    visitor.visit_expr(expression);
    visitor.0
}

struct ConditionBindings(HashSet<String>);

impl<'ast> Visit<'ast> for ConditionBindings {
    fn visit_expr_let(&mut self, expression: &'ast ExprLet) {
        self.0.extend(pattern_bindings(&expression.pat));
        syn::visit::visit_expr_let(self, expression);
    }
}
