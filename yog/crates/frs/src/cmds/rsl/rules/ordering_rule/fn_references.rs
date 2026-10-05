//! Syntactic local references; deliberately no macro expansion.

use proc_macro2::Ident;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use syn::Attribute;
use syn::Block;
use syn::Expr;
use syn::ExprCall;
use syn::ExprPath;
use syn::Item;
use syn::Macro;
use syn::Meta;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit;
use syn::visit::Visit;

#[derive(Clone, Copy)]
pub(super) enum FnPathScope {
    ModuleFns,
    ImplFns,
}

pub(super) fn collect_fn_name_candidates(block: &Block, attributes: &[Attribute], scope: FnPathScope) -> Vec<String> {
    let mut collector = FnNameCandidateCollector {
        scope,
        candidate_names: Vec::new(),
    };
    for attribute in attributes {
        collector.visit_attribute(attribute);
    }
    collector.visit_block(block);
    collector.candidate_names
}

struct FnNameCandidateCollector {
    scope: FnPathScope,
    candidate_names: Vec<String>,
}

impl FnNameCandidateCollector {
    fn scan_attribute_arguments(&mut self, tokens: TokenStream) {
        let Ok(arguments) = Punctuated::<Expr, syn::Token![,]>::parse_terminated.parse2(tokens) else {
            return;
        };
        for expression in &arguments {
            self.visit_expr(expression);
        }
    }

    fn scan_macro_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        let mut remaining = tokens.as_slice();
        loop {
            match remaining {
                [TokenTree::Punct(punct), TokenTree::Ident(_), rest @ ..] if punct.as_char() == '$' => {
                    remaining = rest;
                }
                [TokenTree::Group(group), rest @ ..] => {
                    self.scan_macro_tokens(group.stream());
                    remaining = rest;
                }
                [TokenTree::Punct(first), TokenTree::Punct(second), rest @ ..]
                    if first.as_char() == ':' && second.as_char() == ':' =>
                {
                    // Absolute paths are outside this local-name heuristic.
                    remaining = self::take_identifier_path(rest).1;
                }
                [TokenTree::Ident(_), ..] => {
                    let (segments, rest) = self::take_identifier_path(remaining);
                    let is_macro_invocation =
                        matches!(rest.first(), Some(TokenTree::Punct(punct)) if punct.as_char() == '!');
                    if !is_macro_invocation
                        && let Some(name) = self::match_local_fn_path(segments.into_iter(), self.scope)
                    {
                        self.candidate_names.push(name);
                    }
                    remaining = rest;
                }
                [_, rest @ ..] => remaining = rest,
                [] => break,
            }
        }
    }
}

impl<'ast> Visit<'ast> for FnNameCandidateCollector {
    fn visit_expr_call(&mut self, expression: &'ast ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && let Some(name) = self::local_called_fn_name(path, self.scope)
        {
            self.candidate_names.push(name);
        }
        visit::visit_expr_call(self, expression);
    }

    fn visit_item(&mut self, _item: &'ast Item) {}

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        let mut segments = attribute.path().segments.iter();
        let first = segments.next();
        let is_case_attribute = first.is_some_and(|segment| segment.ident == "case")
            || (first.is_some_and(|segment| segment.ident == "rstest")
                && segments.next().is_some_and(|segment| segment.ident == "case"));
        if is_case_attribute && let Meta::List(list) = &attribute.meta {
            self.scan_attribute_arguments(list.tokens.clone());
        }
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        // Macro tokens are syntactic references, including quoted or discarded arguments.
        self.scan_macro_tokens(mac.tokens.clone());
    }
}

fn take_identifier_path(tokens: &[TokenTree]) -> (Vec<&Ident>, &[TokenTree]) {
    let [TokenTree::Ident(first), rest @ ..] = tokens else {
        return (Vec::new(), tokens);
    };
    let mut remaining = rest;
    let mut segments = vec![first];
    while let [
        TokenTree::Punct(first),
        TokenTree::Punct(second),
        TokenTree::Ident(next),
        rest @ ..,
    ] = remaining
    {
        if first.as_char() != ':' || second.as_char() != ':' {
            break;
        }
        segments.push(next);
        remaining = rest;
    }
    (segments, remaining)
}

fn local_called_fn_name(path: &ExprPath, scope: FnPathScope) -> Option<String> {
    // Caller order is intentionally syntax-only: resolve only local fn paths.
    if path.qself.is_some() || path.path.leading_colon.is_some() {
        return None;
    }
    self::match_local_fn_path(path.path.segments.iter().map(|segment| &segment.ident), scope)
}

fn match_local_fn_path<'a>(mut segments: impl Iterator<Item = &'a Ident>, scope: FnPathScope) -> Option<String> {
    let first = segments.next()?;
    let second = segments.next();
    match (scope, second) {
        (FnPathScope::ModuleFns, None) => Some(first.to_string()),
        (FnPathScope::ModuleFns, Some(second)) if first == "self" && segments.next().is_none() => {
            Some(second.to_string())
        }
        (FnPathScope::ImplFns, Some(second)) if first == "Self" && segments.next().is_none() => {
            Some(second.to_string())
        }
        (FnPathScope::ModuleFns | FnPathScope::ImplFns, _) => None,
    }
}
