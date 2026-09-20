//! Misordered-fn rule for `frs rsl`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use syn::Expr;
use syn::Item;
use syn::visit::Visit;

use crate::cmds::rsl::ast::ItemKind;
use crate::cmds::rsl::ast::ModuleItem;
use crate::cmds::rsl::ast::VisibilityClass;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

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

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug)]
pub(super) struct MisorderedFnViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    details: MisorderedFnDetails,
}

impl MisorderedFnViolation {
    fn new(path: &Path, span: Span, expected_after: String, item: ItemKind) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            details: MisorderedFnDetails { expected_after, item },
        }
    }
}

impl TypedRuleViolation for MisorderedFnViolation {
    type Rule = MisorderedFnRule;
}

impl Display for MisorderedFnViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            MisorderedFnRule::code(),
            &format!(
                "move `{}` after `{}`",
                self.details.item.label(),
                self.details.expected_after
            ),
        ))
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug)]
struct MisorderedFnDetails {
    expected_after: String,
    item: ItemKind,
}

#[derive(Clone, Debug)]
struct FnInfo {
    source_idx: usize,
    span: Span,
    name: String,
    visibility: VisibilityClass,
    calls: Vec<String>,
}

#[derive(Default)]
struct DirectCallCollector {
    associated: bool,
    calls: Vec<String>,
}

impl<'ast> Visit<'ast> for DirectCallCollector {
    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && let Some(name) = self::direct_call_name(path, self.associated)
        {
            self.calls.push(name);
        }
        syn::visit::visit_expr_call(self, expression);
    }

    fn visit_item_fn(&mut self, _fn: &'ast syn::ItemFn) {}
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
        if targets.insert(fn_info.name.clone(), idx).is_some() {
            ambiguous.insert(fn_info.name.clone());
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
    if first_fn.source_idx < anchor_fn.source_idx && anchor_fn.visibility == VisibilityClass::Private {
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
        if helper_fn.source_idx < anchor_fn.source_idx && anchor_fn.visibility == VisibilityClass::Private {
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
        format!("fn {}", expected_after.name),
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
    items
        .iter()
        .enumerate()
        .filter_map(|(source_idx, module_item)| {
            let Item::Fn(fn_item) = module_item.item() else {
                return None;
            };
            Some(FnInfo {
                source_idx,
                span: fn_item.sig.fn_token.span,
                name: fn_item.sig.ident.to_string(),
                visibility: VisibilityClass::from(&fn_item.vis),
                calls: self::direct_calls(&fn_item.block, false),
            })
        })
        .collect()
}

fn impl_fns(item_impl: &syn::ItemImpl) -> Vec<FnInfo> {
    item_impl
        .items
        .iter()
        .enumerate()
        .filter_map(|(source_idx, item)| {
            let syn::ImplItem::Fn(fn_item) = item else {
                return None;
            };
            Some(FnInfo {
                source_idx,
                span: fn_item.sig.fn_token.span,
                name: fn_item.sig.ident.to_string(),
                visibility: VisibilityClass::from(&fn_item.vis),
                calls: self::direct_calls(&fn_item.block, true),
            })
        })
        .collect()
}

fn direct_calls(block: &syn::Block, associated: bool) -> Vec<String> {
    let mut collector = DirectCallCollector {
        associated,
        calls: Vec::new(),
    };
    collector.visit_block(block);
    collector.calls
}

fn direct_call_name(path: &syn::ExprPath, associated: bool) -> Option<String> {
    // Caller order is intentionally syntax-only: resolve only local fn paths.
    if path.qself.is_some() || path.path.leading_colon.is_some() {
        return None;
    }
    let mut segments = path.path.segments.iter();
    let first = segments.next()?;
    if associated {
        if first.ident == "Self"
            && let Some(second) = segments.next()
            && segments.next().is_none()
        {
            return Some(second.ident.to_string());
        }
        None
    } else if segments.next().is_none() {
        Some(first.ident.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::MisorderedFnDetails;
    use super::MisorderedFnRule;
    use super::MisorderedFnViolation;
    use crate::cmds::rsl::ast::ItemKind;
    use crate::cmds::rsl::rules::TypedRule;

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
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 13,
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
                file: PathBuf::from("test.rs"),
                line: 6,
                column: 13,
                details: MisorderedFnDetails {
                    expected_after: "fn first".to_owned(),
                    item: ItemKind::Fn,
                },
            }])
        );
    }

    #[test]
    fn test_misordered_fn_violation_when_details_are_present_formats_compact_output() {
        let violation = MisorderedFnViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 13,
            details: MisorderedFnDetails {
                expected_after: "fn caller".to_owned(),
                item: ItemKind::Fn,
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:2:13,misordered_fn,move `fn` after `fn caller`"
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
