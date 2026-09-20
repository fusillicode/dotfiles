use std::collections::HashSet;

use syn::Item;
use syn::visit::Visit;

struct BindingCollector<'bindings> {
    bindings: &'bindings mut HashSet<String>,
}

impl<'ast> Visit<'ast> for BindingCollector<'_> {
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.bindings.insert(pattern.ident.to_string());
        syn::visit::visit_pat_ident(self, pattern);
    }
}

pub fn add_pattern_bindings(bindings: &mut HashSet<String>, pattern: &syn::Pat) {
    let mut collector = BindingCollector { bindings };
    collector.visit_pat(pattern);
}

pub fn pattern_bindings(pattern: &syn::Pat) -> HashSet<String> {
    let mut bindings = HashSet::new();
    add_pattern_bindings(&mut bindings, pattern);
    bindings
}

pub fn closure_bindings(inputs: &syn::punctuated::Punctuated<syn::Pat, syn::token::Comma>) -> HashSet<String> {
    let mut bindings = HashSet::new();
    for input in inputs {
        add_pattern_bindings(&mut bindings, input);
    }
    bindings
}

pub fn parameter_bindings(inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>) -> HashSet<String> {
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

pub fn block_fn_bindings(block: &syn::Block) -> HashSet<String> {
    block
        .stmts
        .iter()
        .filter_map(|statement| match statement {
            syn::Stmt::Item(Item::Fn(fn_item)) => Some(fn_item.sig.ident.to_string()),
            syn::Stmt::Expr(_, _) | syn::Stmt::Item(_) | syn::Stmt::Local(_) | syn::Stmt::Macro(_) => None,
        })
        .collect()
}
