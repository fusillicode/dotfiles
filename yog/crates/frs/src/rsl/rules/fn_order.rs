//! Function-order rule.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use serde::Serialize;
use syn::Expr;
use syn::Item;
use syn::visit::Visit;

use crate::rsl::ast::ItemKind;
use crate::rsl::ast::VisibilityClass;
use crate::rsl::engine::FileContext;
use crate::rsl::rules::TypedRule;
use crate::rsl::rules::TypedRuleViolation;

pub struct FnOrderRule;

impl TypedRule for FnOrderRule {
    type Violation = FnOrderViolation;

    fn name() -> &'static str {
        "fn_order"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let mut violations = Vec::new();

        for items in crate::rsl::ast::module_scopes(ctx.file) {
            self::check_fn_order(&self::module_functions(items), ctx.path, &mut violations);

            for item in items.iter().rev() {
                if let Item::Impl(item_impl) = item
                    && item_impl.trait_.is_none()
                {
                    self::check_fn_order(&self::impl_functions(item_impl), ctx.path, &mut violations);
                }
            }
        }

        violations
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Serialize)]
pub(super) struct FnOrderViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    message: &'static str,
    details: FnOrderDetails,
}

impl FnOrderViolation {
    fn new(path: &Path, span: Span, expected_after: String, item: ItemKind) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "helper must follow its caller",
            details: FnOrderDetails { expected_after, item },
        }
    }
}

impl TypedRuleViolation for FnOrderViolation {
    type Rule = FnOrderRule;
}

impl Display for FnOrderViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            self.message,
            &format!("{} -> after {}", self.details.item.label(), self.details.expected_after),
        ))
    }
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Debug, Serialize)]
struct FnOrderDetails {
    expected_after: String,
    item: ItemKind,
}

#[derive(Clone, Debug)]
struct FunctionInfo {
    source_index: usize,
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

    fn visit_item_fn(&mut self, _function: &'ast syn::ItemFn) {}
}

fn check_fn_order(functions: &[FunctionInfo], path: &Path, violations: &mut Vec<FnOrderViolation>) {
    if functions.is_empty() {
        return;
    }

    let targets = self::function_targets(functions);
    let callers_by_target = self::callers_by_target(functions, &targets);
    let helper_components = self::helper_components(functions, &targets);
    let mut component_for_helper = vec![None; functions.len()];
    for (component_index, component) in helper_components.iter().enumerate() {
        for &helper in component {
            if let Some(component_slot) = component_for_helper.get_mut(helper) {
                *component_slot = Some(component_index);
            }
        }
    }

    self::check_recursive_helpers(
        functions,
        &targets,
        &callers_by_target,
        &helper_components,
        path,
        violations,
    );
    self::check_non_recursive_helpers(
        functions,
        &targets,
        &callers_by_target,
        &helper_components,
        &component_for_helper,
        path,
        violations,
    );
}

fn function_targets(functions: &[FunctionInfo]) -> HashMap<String, usize> {
    let mut targets = HashMap::new();
    let mut ambiguous = HashSet::new();
    for (index, function) in functions.iter().enumerate() {
        if targets.insert(function.name.clone(), index).is_some() {
            ambiguous.insert(function.name.clone());
        }
    }
    for name in ambiguous {
        targets.remove(&name);
    }
    targets
}

fn callers_by_target(functions: &[FunctionInfo], targets: &HashMap<String, usize>) -> Vec<Vec<usize>> {
    let mut callers_by_target = vec![Vec::new(); functions.len()];
    for (caller, function) in functions.iter().enumerate() {
        for called_name in &function.calls {
            let Some(&target) = targets.get(called_name) else {
                continue;
            };
            let Some(target_function) = functions.get(target) else {
                continue;
            };
            let Some(target_callers) = callers_by_target.get_mut(target) else {
                continue;
            };
            if target_function.visibility != VisibilityClass::Private || target_callers.contains(&caller) {
                continue;
            }
            target_callers.push(caller);
        }
    }
    callers_by_target
}

fn check_recursive_helpers(
    functions: &[FunctionInfo],
    targets: &HashMap<String, usize>,
    callers_by_target: &[Vec<usize>],
    helper_components: &[Vec<usize>],
    path: &Path,
    violations: &mut Vec<FnOrderViolation>,
) {
    for component in helper_components {
        if !self::component_is_recursive(component, functions, targets) {
            continue;
        }
        self::check_recursive_anchor(component, functions, callers_by_target, path, violations);
        self::check_recursive_adjacency(component, functions, path, violations);
    }
}

fn check_recursive_anchor(
    component: &[usize],
    functions: &[FunctionInfo],
    callers_by_target: &[Vec<usize>],
    path: &Path,
    violations: &mut Vec<FnOrderViolation>,
) {
    let members: HashSet<usize> = component.iter().copied().collect();
    let external_callers: Vec<usize> = component
        .iter()
        .flat_map(|&helper| callers_by_target.get(helper).into_iter().flatten().copied())
        .filter(|caller| !members.contains(caller))
        .collect();
    let Some(anchor) = self::caller_anchor(functions, &external_callers, true) else {
        return;
    };
    let Some(&first) = component.iter().min_by_key(|&&helper| {
        functions
            .get(helper)
            .map_or(usize::MAX, |function| function.source_index)
    }) else {
        return;
    };
    let (Some(first_function), Some(anchor_function)) = (functions.get(first), functions.get(anchor)) else {
        return;
    };
    if first_function.source_index < anchor_function.source_index
        && anchor_function.visibility == VisibilityClass::Private
    {
        self::push_caller_violation(first_function, anchor_function, path, violations);
    }
}

fn check_recursive_adjacency(
    component: &[usize],
    functions: &[FunctionInfo],
    path: &Path,
    violations: &mut Vec<FnOrderViolation>,
) {
    let mut ordered = component.to_vec();
    ordered.sort_unstable_by_key(|&helper| {
        functions
            .get(helper)
            .map_or(usize::MAX, |function| function.source_index)
    });
    for pair in ordered.windows(2) {
        let [previous, current] = pair else {
            continue;
        };
        let (Some(previous_function), Some(current_function)) = (functions.get(*previous), functions.get(*current))
        else {
            continue;
        };
        if current_function.source_index != previous_function.source_index.saturating_add(1) {
            self::push_caller_violation(current_function, previous_function, path, violations);
        }
    }
}

fn check_non_recursive_helpers(
    functions: &[FunctionInfo],
    targets: &HashMap<String, usize>,
    callers_by_target: &[Vec<usize>],
    helper_components: &[Vec<usize>],
    component_for_helper: &[Option<usize>],
    path: &Path,
    violations: &mut Vec<FnOrderViolation>,
) {
    for (helper, callers) in callers_by_target.iter().enumerate() {
        let Some(helper_function) = functions.get(helper) else {
            continue;
        };
        let recursive = component_for_helper
            .get(helper)
            .copied()
            .flatten()
            .and_then(|component| helper_components.get(component))
            .is_some_and(|component| self::component_is_recursive(component, functions, targets));
        if helper_function.visibility != VisibilityClass::Private || recursive {
            continue;
        }
        let Some(anchor) = self::caller_anchor(functions, callers, callers.len() > 1) else {
            continue;
        };
        let Some(anchor_function) = functions.get(anchor) else {
            continue;
        };
        if helper_function.source_index < anchor_function.source_index
            && anchor_function.visibility == VisibilityClass::Private
        {
            self::push_caller_violation(helper_function, anchor_function, path, violations);
        }
    }
}

fn push_caller_violation(
    item: &FunctionInfo,
    expected_after: &FunctionInfo,
    path: &Path,
    violations: &mut Vec<FnOrderViolation>,
) {
    violations.push(FnOrderViolation::new(
        path,
        item.span,
        format!("fn {}", expected_after.name),
        ItemKind::Fn,
    ));
}

fn caller_anchor(functions: &[FunctionInfo], callers: &[usize], prefer_public: bool) -> Option<usize> {
    let public_callers = callers.iter().copied().filter(|&caller| {
        functions
            .get(caller)
            .is_some_and(|function| function.visibility == VisibilityClass::Public)
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

    candidates.into_iter().min_by_key(|&caller| {
        functions
            .get(caller)
            .map_or(usize::MAX, |function| function.source_index)
    })
}

fn helper_components(functions: &[FunctionInfo], targets: &HashMap<String, usize>) -> Vec<Vec<usize>> {
    let mut graph = vec![Vec::new(); functions.len()];
    let mut reverse = vec![Vec::new(); functions.len()];
    for (caller, function) in functions.iter().enumerate() {
        if function.visibility != VisibilityClass::Private {
            continue;
        }
        for called_name in &function.calls {
            let Some(&target) = targets.get(called_name) else {
                continue;
            };
            let Some(target_function) = functions.get(target) else {
                continue;
            };
            let Some(caller_edges) = graph.get_mut(caller) else {
                continue;
            };
            if target_function.visibility != VisibilityClass::Private || caller_edges.contains(&target) {
                continue;
            }
            caller_edges.push(target);
            if let Some(target_edges) = reverse.get_mut(target) {
                target_edges.push(caller);
            }
        }
    }

    let mut visited = vec![false; functions.len()];
    let mut order = Vec::new();
    for node in 0..functions.len() {
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

fn component_is_recursive(component: &[usize], functions: &[FunctionInfo], targets: &HashMap<String, usize>) -> bool {
    if component.len() > 1 {
        return true;
    }
    let Some(&helper) = component.first() else {
        return false;
    };
    let Some(function) = functions.get(helper) else {
        return false;
    };
    function
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

fn module_functions(items: &[Item]) -> Vec<FunctionInfo> {
    items
        .iter()
        .enumerate()
        .filter_map(|(source_index, item)| {
            let Item::Fn(function) = item else {
                return None;
            };
            Some(FunctionInfo {
                source_index,
                span: function.sig.fn_token.span,
                name: function.sig.ident.to_string(),
                visibility: VisibilityClass::from(&function.vis),
                calls: self::direct_calls(&function.block, false),
            })
        })
        .collect()
}

fn impl_functions(item_impl: &syn::ItemImpl) -> Vec<FunctionInfo> {
    item_impl
        .items
        .iter()
        .enumerate()
        .filter_map(|(source_index, item)| {
            let syn::ImplItem::Fn(function) = item else {
                return None;
            };
            Some(FunctionInfo {
                source_index,
                span: function.sig.fn_token.span,
                name: function.sig.ident.to_string(),
                visibility: VisibilityClass::from(&function.vis),
                calls: self::direct_calls(&function.block, true),
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
    // Caller order is intentionally syntax-only: resolve only local function paths.
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

    use super::FnOrderDetails;
    use super::FnOrderRule;
    use super::FnOrderViolation;
    use crate::rsl::ast::ItemKind;
    use crate::rsl::rules::TypedRule;

    #[test]
    fn test_fn_order_check_when_private_helper_precedes_caller_reports_helper() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}
            fn caller() {
                helper();
            }
            ",
        )
        .unwrap();

        let result = FnOrderRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![FnOrderViolation {
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 13,
                message: "helper must follow its caller",
                details: FnOrderDetails {
                    expected_after: "fn caller".to_owned(),
                    item: ItemKind::Fn,
                },
            }])
        );
    }

    #[test]
    fn test_fn_order_check_when_mutually_recursive_helpers_are_split_reports_later_helper() {
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

        let result = FnOrderRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![FnOrderViolation {
                file: PathBuf::from("test.rs"),
                line: 6,
                column: 13,
                message: "helper must follow its caller",
                details: FnOrderDetails {
                    expected_after: "fn first".to_owned(),
                    item: ItemKind::Fn,
                },
            }])
        );
    }

    #[test]
    fn test_fn_order_violation_when_details_are_present_formats_compact_output() {
        let violation = FnOrderViolation {
            file: PathBuf::from("test.rs"),
            line: 2,
            column: 13,
            message: "helper must follow its caller",
            details: FnOrderDetails {
                expected_after: "fn caller".to_owned(),
                item: ItemKind::Fn,
            },
        };

        assert_eq!(
            violation.to_string(),
            "test.rs:2:13 helper must follow its caller - fn -> after fn caller"
        );
    }

    #[test]
    fn test_fn_order_check_when_external_module_is_declared_does_not_read_external_file() {
        let syntax = syn::parse_file(
            r"
            mod external;
            fn run() {}
            ",
        )
        .unwrap();

        let result = FnOrderRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }
}
