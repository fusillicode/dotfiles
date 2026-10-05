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
pub(super) enum CallScope {
    Module,
    Associated,
}

pub(super) fn direct_calls(block: &Block, attributes: &[Attribute], scope: CallScope) -> Vec<String> {
    let mut collector = DirectCallCollector {
        scope,
        collected: Vec::new(),
    };
    for attribute in attributes {
        collector.visit_attribute(attribute);
    }
    collector.visit_block(block);
    collector.collected
}

struct DirectCallCollector {
    scope: CallScope,
    collected: Vec<String>,
}

impl DirectCallCollector {
    fn visit_arguments(&mut self, tokens: TokenStream) {
        let Ok(arguments) = Punctuated::<Expr, syn::Token![,]>::parse_terminated.parse2(tokens) else {
            return;
        };
        for expression in &arguments {
            self.visit_expr(expression);
        }
    }

    fn visit_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        let mut remaining = tokens.as_slice();
        loop {
            match remaining {
                [TokenTree::Punct(punct), TokenTree::Ident(_), rest @ ..] if punct.as_char() == '$' => {
                    remaining = rest;
                }
                [TokenTree::Group(group), rest @ ..] => {
                    self.visit_tokens(group.stream());
                    remaining = rest;
                }
                [TokenTree::Punct(first), TokenTree::Punct(second), rest @ ..]
                    if first.as_char() == ':' && second.as_char() == ':' =>
                {
                    // Absolute paths are outside this local-name heuristic.
                    remaining = self::macro_path(rest).1;
                }
                [TokenTree::Ident(_), ..] => {
                    let (segments, rest) = self::macro_path(remaining);
                    let macro_name = matches!(rest.first(), Some(TokenTree::Punct(punct)) if punct.as_char() == '!');
                    if !macro_name && let Some(name) = self::local_call_name(segments.into_iter(), self.scope) {
                        self.collected.push(name);
                    }
                    remaining = rest;
                }
                [_, rest @ ..] => remaining = rest,
                [] => break,
            }
        }
    }
}

impl<'ast> Visit<'ast> for DirectCallCollector {
    fn visit_expr_call(&mut self, expression: &'ast ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && let Some(name) = self::direct_call_name(path, self.scope)
        {
            self.collected.push(name);
        }
        visit::visit_expr_call(self, expression);
    }

    fn visit_item(&mut self, _item: &'ast Item) {}

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        let mut segments = attribute.path().segments.iter();
        let first = segments.next();
        let case = first.is_some_and(|segment| segment.ident == "case")
            || (first.is_some_and(|segment| segment.ident == "rstest")
                && segments.next().is_some_and(|segment| segment.ident == "case"));
        if case && let Meta::List(list) = &attribute.meta {
            self.visit_arguments(list.tokens.clone());
        }
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        // Macro tokens are syntactic references, including quoted or discarded arguments.
        self.visit_tokens(mac.tokens.clone());
    }
}

fn macro_path(tokens: &[TokenTree]) -> (Vec<&Ident>, &[TokenTree]) {
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

fn direct_call_name(path: &ExprPath, scope: CallScope) -> Option<String> {
    // Caller order is intentionally syntax-only: resolve only local fn paths.
    if path.qself.is_some() || path.path.leading_colon.is_some() {
        return None;
    }
    self::local_call_name(path.path.segments.iter().map(|segment| &segment.ident), scope)
}

fn local_call_name<'a>(mut segments: impl Iterator<Item = &'a Ident>, scope: CallScope) -> Option<String> {
    let first = segments.next()?;
    let second = segments.next();
    match (scope, second) {
        (CallScope::Module, None) => Some(first.to_string()),
        (CallScope::Module, Some(second)) if first == "self" && segments.next().is_none() => Some(second.to_string()),
        (CallScope::Associated, Some(second)) if first == "Self" && segments.next().is_none() => {
            Some(second.to_string())
        }
        (CallScope::Module | CallScope::Associated, _) => None,
    }
}
