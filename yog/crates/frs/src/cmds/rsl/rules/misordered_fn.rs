//! Misordered-fn rule for `frs rsl`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use proc_macro2::Ident;
use proc_macro2::Span;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use syn::Attribute;
use syn::Block;
use syn::Expr;
use syn::ExprCall;
use syn::ExprPath;
use syn::ImplItem;
use syn::Item;
use syn::ItemImpl;
use syn::Macro;
use syn::Meta;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit;
use syn::visit::Visit;

use crate::cmds::rsl::ast;
use crate::cmds::rsl::ast::ItemKind;
use crate::cmds::rsl::ast::ModuleItem;
use crate::cmds::rsl::ast::VisibilityClass;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;
use crate::cmds::rsl::rules::common::Location;

pub struct MisorderedFnRule;

impl TypedRule for MisorderedFnRule {
    type Violation = MisorderedFnViolation;

    fn code() -> &'static str {
        "misordered_fn"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let mut violations = Vec::new();

        for items in &ctx.module_item_lists {
            self::check_misordered_fn(&self::module_fns(items), ctx.path, &mut violations);

            for module_item in items.iter().rev() {
                let item = module_item.item();
                if let Item::Impl(item_impl) = item
                    && item_impl.trait_.is_none()
                {
                    self::check_misordered_fn(&self::impl_fns(item_impl), ctx.path, &mut violations);
                }
            }
        }

        violations
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct MisorderedFnViolation {
    pub location: Location,
    pub details: MisorderedFnDetails,
}

impl MisorderedFnViolation {
    fn new(path: &Path, span: Span, expected_after: String, item: ItemKind) -> Self {
        Self {
            location: Location::from_span(path, span),
            details: MisorderedFnDetails { expected_after, item },
        }
    }
}

impl TypedRuleViolation for MisorderedFnViolation {
    type Rule = MisorderedFnRule;
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct MisorderedFnDetails {
    pub expected_after: String,
    pub item: ItemKind,
}

#[derive(Clone, Debug)]
struct FnInfo {
    source_idx: usize,
    span: Span,
    anchor: CallerAnchor,
    visibility: VisibilityClass,
    calls: Vec<String>,
}

impl FnInfo {
    const fn placement_idx(&self) -> usize {
        match &self.anchor {
            CallerAnchor::Function(_) => self.source_idx,
            CallerAnchor::Impl { end_idx, .. } => *end_idx,
        }
    }
}

#[derive(Clone, Debug)]
enum CallerAnchor {
    Function(String),
    Impl { label: String, end_idx: usize },
}

impl CallerAnchor {
    fn label(&self) -> String {
        match self {
            Self::Function(name) => format!("fn {name}"),
            Self::Impl { label, .. } => label.clone(),
        }
    }
}

#[derive(Clone, Copy)]
enum CallScope {
    Module,
    Associated,
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

fn check_misordered_fn(fns: &[FnInfo], path: &Path, violations: &mut Vec<MisorderedFnViolation>) {
    if fns.is_empty() {
        return;
    }

    let targets = self::fn_targets(fns);
    let callers_by_target = self::callers_by_target(fns, &targets);
    let helper_components = self::helper_components(fns, &targets);
    let mut component_for_helper = vec![None; fns.len()];
    for (component_idx, component) in helper_components.iter().enumerate() {
        for &helper in component {
            if let Some(component_slot) = component_for_helper.get_mut(helper) {
                *component_slot = Some(component_idx);
            }
        }
    }

    self::check_recursive_helpers(fns, &targets, &callers_by_target, &helper_components, path, violations);
    self::check_non_recursive_helpers(
        fns,
        &targets,
        &callers_by_target,
        &helper_components,
        &component_for_helper,
        path,
        violations,
    );
}

fn fn_targets(fns: &[FnInfo]) -> HashMap<String, usize> {
    let mut targets = HashMap::new();
    let mut ambiguous = HashSet::new();
    for (idx, fn_info) in fns.iter().enumerate() {
        let CallerAnchor::Function(name) = &fn_info.anchor else {
            continue;
        };
        if targets.insert(name.clone(), idx).is_some() {
            ambiguous.insert(name.clone());
        }
    }
    for name in ambiguous {
        targets.remove(&name);
    }
    targets
}

fn callers_by_target(fns: &[FnInfo], targets: &HashMap<String, usize>) -> Vec<Vec<usize>> {
    let mut callers_by_target = vec![Vec::new(); fns.len()];
    for (caller, fn_info) in fns.iter().enumerate() {
        for called_name in &fn_info.calls {
            let Some(&target) = targets.get(called_name) else {
                continue;
            };
            let Some(target_fn) = fns.get(target) else {
                continue;
            };
            let Some(target_callers) = callers_by_target.get_mut(target) else {
                continue;
            };
            if target_fn.visibility != VisibilityClass::Private || target_callers.contains(&caller) {
                continue;
            }
            target_callers.push(caller);
        }
    }
    callers_by_target
}

fn check_recursive_helpers(
    fns: &[FnInfo],
    targets: &HashMap<String, usize>,
    callers_by_target: &[Vec<usize>],
    helper_components: &[Vec<usize>],
    path: &Path,
    violations: &mut Vec<MisorderedFnViolation>,
) {
    for component in helper_components {
        if !self::component_is_recursive(component, fns, targets) {
            continue;
        }
        self::check_recursive_anchor(component, fns, callers_by_target, path, violations);
        self::check_recursive_adjacency(component, fns, path, violations);
    }
}

fn check_recursive_anchor(
    component: &[usize],
    fns: &[FnInfo],
    callers_by_target: &[Vec<usize>],
    path: &Path,
    violations: &mut Vec<MisorderedFnViolation>,
) {
    let members: HashSet<usize> = component.iter().copied().collect();
    let external_callers: Vec<usize> = component
        .iter()
        .flat_map(|&helper| callers_by_target.get(helper).into_iter().flatten().copied())
        .filter(|caller| !members.contains(caller))
        .collect();
    let Some(anchor) = self::caller_anchor(fns, &external_callers, true) else {
        return;
    };
    let Some(&first) = component
        .iter()
        .min_by_key(|&&helper| fns.get(helper).map_or(usize::MAX, |fn_info| fn_info.source_idx))
    else {
        return;
    };
    let (Some(first_fn), Some(anchor_fn)) = (fns.get(first), fns.get(anchor)) else {
        return;
    };
    if first_fn.source_idx < anchor_fn.placement_idx() && anchor_fn.visibility == VisibilityClass::Private {
        self::push_caller_violation(first_fn, anchor_fn, path, violations);
    }
}

fn check_recursive_adjacency(
    component: &[usize],
    fns: &[FnInfo],
    path: &Path,
    violations: &mut Vec<MisorderedFnViolation>,
) {
    let mut ordered = component.to_vec();
    ordered.sort_unstable_by_key(|&helper| fns.get(helper).map_or(usize::MAX, |fn_info| fn_info.source_idx));
    for pair in ordered.windows(2) {
        let [previous, current] = pair else {
            continue;
        };
        let (Some(previous_fn), Some(current_fn)) = (fns.get(*previous), fns.get(*current)) else {
            continue;
        };
        if current_fn.source_idx != previous_fn.source_idx.saturating_add(1) {
            self::push_caller_violation(current_fn, previous_fn, path, violations);
        }
    }
}

fn check_non_recursive_helpers(
    fns: &[FnInfo],
    targets: &HashMap<String, usize>,
    callers_by_target: &[Vec<usize>],
    helper_components: &[Vec<usize>],
    component_for_helper: &[Option<usize>],
    path: &Path,
    violations: &mut Vec<MisorderedFnViolation>,
) {
    for (helper, callers) in callers_by_target.iter().enumerate() {
        let Some(helper_fn) = fns.get(helper) else {
            continue;
        };
        let recursive = component_for_helper
            .get(helper)
            .copied()
            .flatten()
            .and_then(|component| helper_components.get(component))
            .is_some_and(|component| self::component_is_recursive(component, fns, targets));
        if helper_fn.visibility != VisibilityClass::Private || recursive {
            continue;
        }
        let Some(anchor) = self::caller_anchor(fns, callers, callers.len() > 1) else {
            continue;
        };
        let Some(anchor_fn) = fns.get(anchor) else {
            continue;
        };
        if helper_fn.source_idx < anchor_fn.placement_idx() && anchor_fn.visibility == VisibilityClass::Private {
            self::push_caller_violation(helper_fn, anchor_fn, path, violations);
        }
    }
}

fn push_caller_violation(
    item: &FnInfo,
    expected_after: &FnInfo,
    path: &Path,
    violations: &mut Vec<MisorderedFnViolation>,
) {
    violations.push(MisorderedFnViolation::new(
        path,
        item.span,
        expected_after.anchor.label(),
        ItemKind::Fn,
    ));
}

fn caller_anchor(fns: &[FnInfo], callers: &[usize], prefer_public: bool) -> Option<usize> {
    let public_callers = callers.iter().copied().filter(|&caller| {
        fns.get(caller)
            .is_some_and(|fn_info| fn_info.visibility == VisibilityClass::Public)
    });
    let candidates = if prefer_public {
        let public_callers: Vec<_> = public_callers.collect();
        if public_callers.is_empty() {
            callers.to_vec()
        } else {
            public_callers
        }
    } else {
        callers.to_vec()
    };

    candidates
        .into_iter()
        .min_by_key(|&caller| fns.get(caller).map_or(usize::MAX, |fn_info| fn_info.source_idx))
}

fn helper_components(fns: &[FnInfo], targets: &HashMap<String, usize>) -> Vec<Vec<usize>> {
    let mut graph = vec![Vec::new(); fns.len()];
    let mut reverse = vec![Vec::new(); fns.len()];
    for (caller, fn_info) in fns.iter().enumerate() {
        if fn_info.visibility != VisibilityClass::Private {
            continue;
        }
        for called_name in &fn_info.calls {
            let Some(&target) = targets.get(called_name) else {
                continue;
            };
            let Some(target_fn) = fns.get(target) else {
                continue;
            };
            let Some(caller_edges) = graph.get_mut(caller) else {
                continue;
            };
            if target_fn.visibility != VisibilityClass::Private || caller_edges.contains(&target) {
                continue;
            }
            caller_edges.push(target);
            if let Some(target_edges) = reverse.get_mut(target) {
                target_edges.push(caller);
            }
        }
    }

    let mut visited = vec![false; fns.len()];
    let mut order = Vec::new();
    for node in 0..fns.len() {
        self::visit_graph(node, &graph, &mut visited, &mut order);
    }

    visited.fill(false);
    let mut components = Vec::new();
    for node in order.into_iter().rev() {
        if visited.get(node).copied().unwrap_or(false) {
            continue;
        }
        let mut component = Vec::new();
        self::collect_graph_component(node, &reverse, &mut visited, &mut component);
        components.push(component);
    }

    components
}

fn component_is_recursive(component: &[usize], fns: &[FnInfo], targets: &HashMap<String, usize>) -> bool {
    if component.len() > 1 {
        return true;
    }
    let Some(&helper) = component.first() else {
        return false;
    };
    let Some(fn_info) = fns.get(helper) else {
        return false;
    };
    fn_info
        .calls
        .iter()
        .filter_map(|called_name| targets.get(called_name))
        .any(|&target| target == helper)
}

fn visit_graph(node: usize, graph: &[Vec<usize>], visited: &mut [bool], order: &mut Vec<usize>) {
    let mut stack = vec![(node, false)];
    while let Some((current, expanded)) = stack.pop() {
        if expanded {
            order.push(current);
            continue;
        }
        if visited.get(current).copied().unwrap_or(false) {
            continue;
        }
        let Some(visited_node) = visited.get_mut(current) else {
            continue;
        };
        *visited_node = true;
        stack.push((current, true));
        if let Some(next_nodes) = graph.get(current) {
            for &next in next_nodes.iter().rev() {
                stack.push((next, false));
            }
        }
    }
}

fn collect_graph_component(node: usize, graph: &[Vec<usize>], visited: &mut [bool], component: &mut Vec<usize>) {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if visited.get(current).copied().unwrap_or(false) {
            continue;
        }
        let Some(visited_node) = visited.get_mut(current) else {
            continue;
        };
        *visited_node = true;
        component.push(current);
        if let Some(next_nodes) = graph.get(current) {
            stack.extend(next_nodes.iter().rev().copied());
        }
    }
}

fn module_fns(items: &[ModuleItem<'_>]) -> Vec<FnInfo> {
    let mut fns: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(source_idx, module_item)| {
            let Item::Fn(fn_item) = module_item.item() else {
                return None;
            };
            Some(FnInfo {
                source_idx,
                span: fn_item.sig.fn_token.span,
                anchor: CallerAnchor::Function(fn_item.sig.ident.to_string()),
                visibility: VisibilityClass::from(&fn_item.vis),
                calls: self::direct_calls(&fn_item.block, &fn_item.attrs, CallScope::Module),
            })
        })
        .collect();

    for node in ast::module_nodes(items) {
        let Some(&end_idx) = node.idxs.iter().max() else {
            continue;
        };
        let Some(last_item) = items.get(end_idx) else {
            continue;
        };
        let mut calls = Vec::new();
        let mut has_impl = false;
        for idx in &node.idxs {
            let Some(Item::Impl(item_impl)) = items.get(*idx).map(ModuleItem::item) else {
                continue;
            };
            has_impl = true;
            for item in &item_impl.items {
                let ImplItem::Fn(method) = item else {
                    continue;
                };
                let method_calls = self::direct_calls(&method.block, &method.attrs, CallScope::Module);
                calls.extend(method_calls);
            }
        }
        if has_impl {
            // A module helper belongs after the whole cluster, never between a type and its impls.
            fns.push(FnInfo {
                source_idx: node.order.source_idx,
                span: ast::item_span(last_item.item()),
                anchor: CallerAnchor::Impl {
                    label: ast::impl_order_label(last_item.item()),
                    end_idx,
                },
                // Module visibility ordering applies to the containing type, rather than its methods.
                visibility: node.order.visibility.unwrap_or(VisibilityClass::Private),
                calls,
            });
        }
    }
    fns
}

fn impl_fns(item_impl: &ItemImpl) -> Vec<FnInfo> {
    item_impl
        .items
        .iter()
        .enumerate()
        .filter_map(|(source_idx, item)| {
            let ImplItem::Fn(fn_item) = item else {
                return None;
            };
            Some(FnInfo {
                source_idx,
                span: fn_item.sig.fn_token.span,
                anchor: CallerAnchor::Function(fn_item.sig.ident.to_string()),
                visibility: VisibilityClass::from(&fn_item.vis),
                calls: self::direct_calls(&fn_item.block, &fn_item.attrs, CallScope::Associated),
            })
        })
        .collect()
}

fn direct_calls(block: &Block, attributes: &[Attribute], scope: CallScope) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use crate::cmds::rsl::ast::ItemKind;
    use crate::cmds::rsl::rules::TypedRule;
    use crate::cmds::rsl::rules::common::Location;
    use crate::cmds::rsl::rules::misordered_fn::MisorderedFnDetails;
    use crate::cmds::rsl::rules::misordered_fn::MisorderedFnRule;
    use crate::cmds::rsl::rules::misordered_fn::MisorderedFnViolation;

    #[rstest::rstest]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                vec![helper(); 3];
            }
            "
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                stringify!(helper());
            }
            "
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                custom!(nested => [self::helper()]);
            }
            "
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                custom!(helper);
            }
            "
    )]
    #[case(
        r"
            fn helper() {}

            fn unknown() {
                run_helper!();
            }

            fn caller() {
                helper();
            }
            "
    )]
    fn test_misordered_fn_check_when_macro_tokens_reference_helper_reports_syntactic_caller(#[case] source: &str) {
        let syntax = syn::parse_file(source).unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_eq!(
            result,
            vec![MisorderedFnViolation {
                location: Location::new(PathBuf::from("test.rs"), 2, 13),
                details: MisorderedFnDetails {
                    expected_after: "fn caller".to_owned(),
                    item: ItemKind::Fn,
                },
            }]
        );
    }

    #[rstest::rstest]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                run_helper!();
            }
            "
    )]
    #[case(
        r#"
            fn helper() {}

            fn caller() {
                custom!("helper()");
            }
            "#
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                custom!(
                    foreign::helper(),
                    ::helper(),
                    self::nested::helper(),
                    foreign::Namespace::<T>::helper()
                );
            }
            "
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                custom!($helper);
            }
            "
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                custom!(helper!());
            }
            "
    )]
    fn test_misordered_fn_check_when_macro_tokens_have_no_local_reference_avoids_inferred_call(#[case] source: &str) {
        let syntax = syn::parse_file(source).unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_eq!(result, Vec::new());
    }

    #[rstest::rstest]
    #[case(
        r"
            fn early() {
                vec![helper(); 3];
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            fn early() {
                stringify!(helper());
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    fn test_misordered_fn_check_when_macro_reference_precedes_helper_preserves_earliest_anchor(#[case] source: &str) {
        let syntax = syn::parse_file(source).unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_eq!(result, Vec::new());
    }

    #[test]
    fn test_misordered_fn_check_when_custom_macro_references_associated_helper_reports_method() {
        let syntax = syn::parse_file(
            r"
            impl Data {
                fn helper() {}

                fn caller() {
                    custom!(Self::helper);
                }
            }
            ",
        )
        .unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_eq!(
            result,
            vec![MisorderedFnViolation {
                location: Location::new(PathBuf::from("test.rs"), 3, 17),
                details: MisorderedFnDetails {
                    expected_after: "fn caller".to_owned(),
                    item: ItemKind::Fn,
                },
            }]
        );
    }

    #[rstest::rstest]
    #[case(
        r"
            #[case(helper())]
            fn early() {}

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            #[case::named(helper())]
            fn early() {}

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            fn early() {
                assert!(helper());
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            fn early() {
                test_that::assert_that!(helper(), eq(true));
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            fn early() {
                assert_eq!(helper(), true);
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            fn early() {
                self::helper();
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            fn early() {
                opaque!(helper);
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            fn early() {
                assert!(helper() => not Rust syntax);
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            struct Data;

            impl Data {
                pub fn run() {
                    helper();
                }
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            struct Data;

            impl Behavior for Data {
                fn run() {
                    self::helper();
                }
            }

            fn helper() {}

            fn late() {
                helper();
            }
            "
    )]
    #[case(
        r"
            fn early() {
                fn nested() {
                    helper();
                }
            }

            fn helper() {}

            fn late() {
                super::helper();
            }
            "
    )]
    #[case(
        r"
            impl Data {
                fn early() {
                    Self::helper();
                }

                fn helper() {}

                fn late() {
                    Self::helper();
                }
            }
            "
    )]
    #[case(
        r"
            impl Data {
                fn early() {
                    assert!(Self::helper());
                }

                fn helper() {}

                fn late() {
                    Self::helper();
                }
            }
            "
    )]
    #[case(
        r"
            fn early() {
                fn nested() {
                    opaque!();
                }
            }

            fn helper() {}
            "
    )]
    #[case(
        r"
            fn early() {
                crate::helper();
            }

            fn helper() {}
            "
    )]
    #[case(
        r"
            fn early() {
                self::nested::helper();
            }

            fn helper() {}
            "
    )]
    #[case(
        r"
            fn early() {
                Self::helper();
            }

            fn helper() {}
            "
    )]
    #[case(
        r"
            fn helper() {}

            impl Data {
                fn run() {
                    Self::helper();
                }
            }
            "
    )]
    #[case(
        r"
            fn helper() {}

            pub struct Data;

            impl Data {
                pub fn run() {
                    helper();
                }
            }
            "
    )]
    fn test_misordered_fn_check_when_existing_order_has_no_detected_violation_avoids_move(#[case] source: &str) {
        let syntax = syn::parse_file(source).unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_eq!(result, Vec::new());
    }

    #[rstest::rstest]
    #[case(
        r"
            fn helper() {}

            #[case(helper())]
            fn caller() {}
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                assert!(helper());
            }
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                assert_that!(helper(), eq(true));
            }
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                self::helper();
            }
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            struct Data;

            impl Data {
                fn run() {
                    helper();
                }
            }
            ",
        "inherent impl Data"
    )]
    #[case(
        r"
            fn helper() {}

            struct Data;

            impl Data {
                pub fn run() {
                    helper();
                }
            }

            impl Behavior for Data {}
            ",
        "trait impl Data"
    )]
    #[case(
        r"
            fn helper() {}

            impl External {
                fn run() {
                    helper();
                }
            }
            ",
        "inherent impl External"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                helper();
            }

            fn later() {
                opaque!();
            }
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                assert!(helper());
            }

            fn late() {
                helper();
            }
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            #[case(helper())]
            fn caller() {}

            fn late() {
                helper();
            }
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            struct Data;

            impl Data {}

            impl Behavior for Data {
                fn run() {
                    helper();
                }
            }
            ",
        "trait impl Data"
    )]
    #[case(
        r"
            fn helper() {}

            #[rstest::case(helper())]
            fn caller() {}
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                core::assert!(helper());
            }
            ",
        "fn caller"
    )]
    #[case(
        r#"
            fn helper() {}

            fn caller() {
                std::assert_eq!(helper(), 1, "{}", helper());
            }
            "#,
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                fn nested() {
                    opaque!();
                }
                helper();
            }
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            struct Data;

            fn late() {
                helper();
            }

            impl Data {
                fn run() {
                    helper();
                }
            }
            ",
        "inherent impl Data"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                assert!(opaque!());
                helper();
            }
            ",
        "fn caller"
    )]
    #[case(
        r"
            fn helper() {}

            fn caller() {
                other::assert!(helper());
            }
            ",
        "fn caller"
    )]
    fn test_misordered_fn_check_when_helper_precedes_detected_caller_reports_anchor(
        #[case] source: &str,
        #[case] expected_after: &str,
    ) {
        let syntax = syn::parse_file(source).unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_eq!(
            result,
            vec![MisorderedFnViolation {
                location: Location::new(PathBuf::from("test.rs"), 2, 13),
                details: MisorderedFnDetails {
                    expected_after: expected_after.to_owned(),
                    item: ItemKind::Fn,
                },
            }]
        );
    }

    #[test]
    fn test_misordered_fn_check_when_helper_follows_complete_cluster_preserves_impl_adjacency() {
        let syntax = syn::parse_file(
            r"
            struct Data;

            impl Data {
                fn run() {
                    helper();
                }
            }

            impl Behavior for Data {}

            fn helper() {}

            fn late() {
                helper();
            }
            ",
        )
        .unwrap();
        let ctx = crate::cmds::rsl::rules::test_ctx(&syntax);

        let caller_findings = MisorderedFnRule.check(&ctx);
        let adjacency_findings = crate::cmds::rsl::rules::nonadjacent_impl::NonadjacentImplRule.check(&ctx);

        assert_eq!(caller_findings, Vec::new());
        assert_eq!(adjacency_findings, Vec::new());
    }

    #[test]
    fn test_misordered_fn_check_when_recursive_calls_are_in_assertions_reports_split_helpers() {
        let syntax = syn::parse_file(
            r"
            fn first() {
                assert!(second());
            }

            fn gap() {}

            fn second() {
                assert!(first());
            }
            ",
        )
        .unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_eq!(
            result,
            vec![MisorderedFnViolation {
                location: Location::new(PathBuf::from("test.rs"), 8, 13),
                details: MisorderedFnDetails {
                    expected_after: "fn first".to_owned(),
                    item: ItemKind::Fn,
                },
            }]
        );
    }

    #[test]
    fn test_misordered_fn_check_when_private_helper_precedes_caller_reports_helper() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}
            fn caller() {
                helper();
            }
            ",
        )
        .unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedFnViolation {
                location: Location::new(PathBuf::from("test.rs"), 2, 13),
                details: MisorderedFnDetails {
                    expected_after: "fn caller".to_owned(),
                    item: ItemKind::Fn,
                },
            }])
        );
    }

    #[test]
    fn test_misordered_fn_check_when_mutually_recursive_helpers_are_split_reports_later_helper() {
        let syntax = syn::parse_file(
            r"
            fn first() {
                second();
            }
            fn gap() {}
            fn second() {
                first();
            }
            ",
        )
        .unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![MisorderedFnViolation {
                location: Location::new(PathBuf::from("test.rs"), 6, 13),
                details: MisorderedFnDetails {
                    expected_after: "fn first".to_owned(),
                    item: ItemKind::Fn,
                },
            }])
        );
    }

    #[test]
    fn test_misordered_fn_check_when_external_module_is_declared_does_not_read_external_file() {
        let syntax = syn::parse_file(
            r"
            mod external;
            fn run() {}
            ",
        )
        .unwrap();

        let result = MisorderedFnRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }
}
