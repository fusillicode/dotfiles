//! Atomic blocks and precedence relationships, with stable source-order ties.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use proc_macro2::TokenStream;
use syn::Attribute;
use syn::ImplItem;
use syn::Item;
use syn::ItemMod;
use syn::Macro;
use syn::Meta;
use syn::Token;
use syn::punctuated::Punctuated;
use syn::visit;
use syn::visit::Visit;

use super::ItemAst;
use super::OrderingConstraint;
use super::OrderingConstraintKind;
use super::OrderingItem;
use super::OrderingOutcome;
use super::ast;
use super::fn_references;
use super::fn_references::FnPathScope;
use crate::cmds::rsl::ast::ItemGroup;
use crate::cmds::rsl::ast::VisibilityClass;

pub(super) fn compute_required_item_order(items: &mut [OrderingItem<'_>]) -> OrderingOutcome {
    let mut blocks = AtomicItemBlocks::new(items);
    if let Err(message) = merge_type_impl_blocks(items, &mut blocks) {
        return OrderingOutcome::Failed {
            message,
            constraints: blocks.constraints,
        };
    }

    let fn_indices_by_name = unique_fn_indices_by_name(items);
    let helper_call_graph = build_private_helper_call_graph(items, &fn_indices_by_name);
    merge_recursive_fn_blocks(items, &helper_call_graph, &mut blocks);
    for members in &blocks.members {
        if let Some(&block_first_item_index) = members.first() {
            for &member_item_index in members {
                if let Some(item) = items.get_mut(member_item_index) {
                    item.metadata.block_first_item_index = block_first_item_index;
                }
            }
        }
    }

    let mut precedence_graph = BlockPrecedenceGraph::new(&blocks);
    add_macro_source_order_constraints(items, &blocks, &mut precedence_graph);
    add_section_group_visibility_constraints(items, &blocks, &mut precedence_graph);
    add_caller_before_helper_constraints(items, &blocks, &helper_call_graph, &mut precedence_graph);

    let required_block_order = precedence_graph.stable_topological_order(&blocks);
    let required_block_order = match required_block_order {
        Ok(required_block_order) => required_block_order,
        Err(unresolved_blocks) => {
            let cycle_labels: Vec<_> = precedence_graph
                .find_cycle(&unresolved_blocks)
                .iter()
                .filter_map(|&block_index| {
                    blocks
                        .members
                        .get(block_index)
                        .and_then(|members| members.first())
                        .and_then(|&item_index| items.get(item_index))
                })
                .map(|item| format!("{}@{}", item.metadata.label, item.metadata.range.format_compact()))
                .collect();
            return OrderingOutcome::Failed {
                message: format!("cycle [{}]", cycle_labels.join(" -> ")),
                constraints: blocks
                    .constraints
                    .into_iter()
                    .chain(precedence_graph.constraints)
                    .collect(),
            };
        }
    };

    let constraints = std::mem::take(&mut blocks.constraints)
        .into_iter()
        .chain(precedence_graph.constraints)
        .collect();

    let required_item_order = blocks.expand_block_order_to_item_order(&required_block_order);
    let source_item_order: Vec<_> = (0..items.len()).collect();

    OrderingOutcome::Computed {
        source_item_order,
        required_item_order,
        constraints,
    }
}

struct AtomicItemBlocks {
    members: Vec<Vec<usize>>,
    constraints: Vec<OrderingConstraint>,
}

impl AtomicItemBlocks {
    fn new(items: &[OrderingItem<'_>]) -> Self {
        Self {
            members: (0..items.len()).map(|index| vec![index]).collect(),
            constraints: Vec::new(),
        }
    }

    fn block_index_containing_item(&self, item_index: usize) -> Option<usize> {
        self.members.iter().position(|members| members.contains(&item_index))
    }

    fn merge(&mut self, members: Vec<usize>, kind: OrderingConstraintKind) {
        for pair in members.windows(2) {
            if let [before_item_index, after_item_index] = pair {
                self.constraints.push(OrderingConstraint {
                    before_item_index: *before_item_index,
                    after_item_index: *after_item_index,
                    kind,
                });
            }
        }
        self.members
            .retain(|block| !block.iter().any(|item| members.contains(item)));
        self.members.push(members);
        self.members.sort_by_key(|block| block.iter().min().copied());
    }

    fn expand_block_order_to_item_order(&self, block_order: &[usize]) -> Vec<usize> {
        block_order
            .iter()
            .filter_map(|&block_index| self.members.get(block_index))
            .flatten()
            .copied()
            .collect()
    }
}

enum MacroOrderingRole {
    Source,
    PossibleConsumer,
    Unrelated,
}

impl From<ItemAst<'_>> for MacroOrderingRole {
    fn from(ast: ItemAst<'_>) -> Self {
        let is_source = match ast {
            ItemAst::ModuleItem(Item::Macro(_) | Item::Verbatim(_))
            | ItemAst::ImplItem(ImplItem::Macro(_) | ImplItem::Verbatim(_)) => true,
            ItemAst::ModuleItem(Item::Mod(module)) => module
                .attrs
                .iter()
                .any(|attribute| is_macro_import_attribute(&attribute.meta)),
            ItemAst::ModuleItem(Item::ExternCrate(item)) => item
                .attrs
                .iter()
                .any(|attribute| is_macro_import_attribute(&attribute.meta)),
            ItemAst::ModuleItem(_) | ItemAst::ImplItem(_) => false,
        };
        if is_source {
            return Self::Source;
        }

        let mut consumer = PossibleMacroConsumer::default();
        match ast {
            ItemAst::ModuleItem(item) => consumer.visit_item(item),
            ItemAst::ImplItem(item) => consumer.visit_impl_item(item),
        }
        if consumer.found {
            Self::PossibleConsumer
        } else {
            Self::Unrelated
        }
    }
}

#[derive(Default)]
struct PossibleMacroConsumer {
    found: bool,
}

impl<'ast> Visit<'ast> for PossibleMacroConsumer {
    fn visit_macro(&mut self, _: &'ast Macro) {
        self.found = true;
    }

    fn visit_attribute(&mut self, _: &'ast Attribute) {
        self.found = true;
    }

    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        if module.content.is_none() {
            self.found = true;
        } else {
            visit::visit_item_mod(self, module);
        }
    }

    fn visit_token_stream(&mut self, _: &'ast TokenStream) {
        self.found = true;
    }
}

fn add_macro_source_order_constraints(
    items: &[OrderingItem<'_>],
    blocks: &AtomicItemBlocks,
    precedence_graph: &mut BlockPrecedenceGraph,
) {
    let roles: Vec<_> = items.iter().map(|item| MacroOrderingRole::from(item.ast)).collect();
    for (before_item_index, before_role) in roles.iter().enumerate() {
        for (after_item_index, after_role) in roles.iter().enumerate().skip(before_item_index.saturating_add(1)) {
            let preserves_binding = matches!(
                (before_role, after_role),
                (
                    MacroOrderingRole::Source,
                    MacroOrderingRole::Source | MacroOrderingRole::PossibleConsumer
                ) | (MacroOrderingRole::PossibleConsumer, MacroOrderingRole::Source)
            );
            if !preserves_binding {
                continue;
            }
            if let (Some(before_block), Some(after_block)) = (
                blocks.block_index_containing_item(before_item_index),
                blocks.block_index_containing_item(after_item_index),
            ) {
                precedence_graph.add_precedence(
                    before_block,
                    after_block,
                    OrderingConstraintKind::MacroBindingPreservation,
                    blocks,
                );
            }
        }
    }
}

fn is_macro_import_attribute(meta: &Meta) -> bool {
    if meta.path().is_ident("macro_use") {
        return true;
    }
    let Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        .is_ok_and(|arguments| arguments.iter().skip(1).any(is_macro_import_attribute))
}

struct BlockPrecedenceEdge {
    successor_block_index: usize,
    kind: OrderingConstraintKind,
}

struct BlockPrecedenceGraph {
    outgoing_edges: Vec<Vec<BlockPrecedenceEdge>>,
    constraints: Vec<OrderingConstraint>,
}

impl BlockPrecedenceGraph {
    fn new(blocks: &AtomicItemBlocks) -> Self {
        Self {
            outgoing_edges: (0..blocks.members.len()).map(|_| Vec::new()).collect(),
            constraints: Vec::new(),
        }
    }

    fn add_precedence(
        &mut self,
        before_block_index: usize,
        after_block_index: usize,
        kind: OrderingConstraintKind,
        blocks: &AtomicItemBlocks,
    ) {
        if before_block_index == after_block_index {
            return;
        }

        if matches!(
            kind,
            OrderingConstraintKind::GroupOrTestSectionOrder
                | OrderingConstraintKind::VisibilityOrder
                | OrderingConstraintKind::CallerBeforeHelper
        ) && self.has_macro_preserving_path(after_block_index, before_block_index)
        {
            return;
        }

        if let Some(outgoing_edges) = self.outgoing_edges.get_mut(before_block_index)
            && !outgoing_edges
                .iter()
                .any(|edge| edge.successor_block_index == after_block_index)
        {
            outgoing_edges.push(BlockPrecedenceEdge {
                successor_block_index: after_block_index,
                kind,
            });
            if let (Some(before_item_index), Some(after_item_index)) = (
                blocks.members.get(before_block_index).and_then(|m| m.last()),
                blocks.members.get(after_block_index).and_then(|m| m.first()),
            ) {
                self.constraints.push(OrderingConstraint {
                    before_item_index: *before_item_index,
                    after_item_index: *after_item_index,
                    kind,
                });
            }
        }
    }

    fn has_macro_preserving_path(&self, start: usize, target: usize) -> bool {
        let mut pending = VecDeque::from([(start, false)]);
        let mut visited = HashSet::from([(start, false)]);
        while let Some((block_index, preserves_macro_order)) = pending.pop_front() {
            for edge in self.outgoing_edges.get(block_index).into_iter().flatten() {
                let preserves_macro_order =
                    preserves_macro_order || matches!(edge.kind, OrderingConstraintKind::MacroBindingPreservation);
                let successor = (edge.successor_block_index, preserves_macro_order);
                if successor == (target, true) {
                    return true;
                }
                if visited.insert(successor) {
                    pending.push_back(successor);
                }
            }
        }
        false
    }

    fn stable_topological_order(&self, blocks: &AtomicItemBlocks) -> Result<Vec<usize>, Vec<usize>> {
        let mut incoming_edge_counts = vec![0_usize; self.outgoing_edges.len()];
        for edge in self.outgoing_edges.iter().flatten() {
            if let Some(count) = incoming_edge_counts.get_mut(edge.successor_block_index) {
                *count = count.saturating_add(1);
            }
        }

        let earliest_source_item_index = |block_index: usize| {
            blocks
                .members
                .get(block_index)
                .and_then(|members| members.iter().min())
                .copied()
        };

        let mut ready_blocks: BTreeSet<_> = incoming_edge_counts
            .iter()
            .enumerate()
            .filter(|(_, count)| **count == 0)
            .map(|(block_index, _)| (earliest_source_item_index(block_index), block_index))
            .collect();

        let mut block_order = Vec::new();
        while let Some((_, next_block_index)) = ready_blocks.pop_first() {
            block_order.push(next_block_index);
            for edge in self.outgoing_edges.get(next_block_index).into_iter().flatten() {
                if let Some(count) = incoming_edge_counts.get_mut(edge.successor_block_index) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready_blocks.insert((
                            earliest_source_item_index(edge.successor_block_index),
                            edge.successor_block_index,
                        ));
                    }
                }
            }
        }

        if block_order.len() == self.outgoing_edges.len() {
            return Ok(block_order);
        }

        Err(incoming_edge_counts
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(block_index, _)| block_index)
            .collect())
    }

    fn find_cycle(&self, unresolved_blocks: &[usize]) -> Vec<usize> {
        for &start_block_index in unresolved_blocks {
            let mut pending_paths = VecDeque::from([vec![start_block_index]]);
            let mut visited = HashSet::from([start_block_index]);
            while let Some(path) = pending_paths.pop_front() {
                let Some(&last_block_index) = path.last() else {
                    continue;
                };

                for edge in self.outgoing_edges.get(last_block_index).into_iter().flatten() {
                    let next_block_index = edge.successor_block_index;
                    if !unresolved_blocks.contains(&next_block_index) {
                        continue;
                    }
                    let mut extended_path = path.clone();
                    extended_path.push(next_block_index);

                    if next_block_index == start_block_index {
                        return extended_path;
                    }

                    if visited.insert(next_block_index) {
                        pending_paths.push_back(extended_path);
                    }
                }
            }
        }

        Vec::new()
    }
}

fn merge_type_impl_blocks(items: &[OrderingItem<'_>], blocks: &mut AtomicItemBlocks) -> Result<(), String> {
    let mut declaration_indices_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (source_item_index, item) in items.iter().enumerate() {
        if let ItemAst::ModuleItem(item) = item.ast
            && let Some(name) = ast::type_definition_name(item)
        {
            declaration_indices_by_name
                .entry(name)
                .or_default()
                .push(source_item_index);
        }
    }

    let mut impl_indices_by_declaration: HashMap<usize, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (source_item_index, item) in items.iter().enumerate() {
        let ItemAst::ModuleItem(Item::Impl(item)) = item.ast else {
            continue;
        };
        let Some(name) = ast::impl_target_name(item) else {
            continue;
        };
        let Some(candidate_declarations) = declaration_indices_by_name.get(&name) else {
            continue;
        };

        let selected_declaration_index = if candidate_declarations.len() == 1 {
            candidate_declarations.first().copied()
        } else {
            candidate_declarations
                .iter()
                .rev()
                .copied()
                .find(|&selected_declaration_index| selected_declaration_index < source_item_index)
        };

        let Some(selected_declaration_index) = selected_declaration_index else {
            return Err(format!(
                "impl {name} has multiple declarations and no preceding declaration"
            ));
        };

        let (inherent_impl_indices, trait_impl_indices) = impl_indices_by_declaration
            .entry(selected_declaration_index)
            .or_default();

        if item.trait_.is_none() {
            inherent_impl_indices.push(source_item_index);
        } else {
            trait_impl_indices.push(source_item_index);
        }
    }

    let mut impl_indices_by_declaration: Vec<_> = impl_indices_by_declaration.into_iter().collect();
    impl_indices_by_declaration.sort_by_key(|(selected_declaration_index, _)| *selected_declaration_index);

    for (selected_declaration_index, (inherent_impl_indices, trait_impl_indices)) in impl_indices_by_declaration {
        let members = std::iter::once(selected_declaration_index)
            .chain(inherent_impl_indices)
            .chain(trait_impl_indices)
            .collect();
        blocks.merge(members, OrderingConstraintKind::TypeImplAdjacency);
    }

    Ok(())
}

fn unique_fn_indices_by_name(items: &[OrderingItem<'_>]) -> HashMap<String, usize> {
    let mut fn_indices_by_name = HashMap::new();
    let mut ambiguous_fn_names = HashSet::new();
    for (source_item_index, item) in items.iter().enumerate() {
        if let Some(name) = item.ast.fn_name().map(ToString::to_string)
            && fn_indices_by_name.insert(name.clone(), source_item_index).is_some()
        {
            ambiguous_fn_names.insert(name);
        }
    }
    for name in ambiguous_fn_names {
        fn_indices_by_name.remove(&name);
    }
    fn_indices_by_name
}

fn build_private_helper_call_graph(
    items: &[OrderingItem<'_>],
    fn_indices_by_name: &HashMap<String, usize>,
) -> Vec<Vec<usize>> {
    items
        .iter()
        .map(|item| {
            let mut callee_item_indices: Vec<_> = collect_item_fn_name_candidates(item)
                .iter()
                .filter_map(|name| fn_indices_by_name.get(name).copied())
                .filter(|&callee_item_index| {
                    items.get(callee_item_index).is_some_and(|callee_item| {
                        callee_item.section == item.section
                            && callee_item.metadata.visibility == Some(VisibilityClass::Private)
                    })
                })
                .collect();
            callee_item_indices.sort_unstable();
            callee_item_indices.dedup();
            callee_item_indices
        })
        .collect()
}

fn collect_item_fn_name_candidates(item: &OrderingItem<'_>) -> Vec<String> {
    match item.ast {
        ItemAst::ModuleItem(Item::Fn(item)) => {
            return fn_references::collect_fn_name_candidates(&item.block, &item.attrs, FnPathScope::ModuleFns);
        }
        ItemAst::ImplItem(ImplItem::Fn(item)) => {
            return fn_references::collect_fn_name_candidates(&item.block, &item.attrs, FnPathScope::ImplFns);
        }
        ItemAst::ModuleItem(_) | ItemAst::ImplItem(_) => {}
    }
    let ItemAst::ModuleItem(Item::Impl(item)) = item.ast else {
        return Vec::new();
    };

    item.items
        .iter()
        .filter_map(|item| {
            let ImplItem::Fn(method) = item else {
                return None;
            };
            Some(fn_references::collect_fn_name_candidates(
                &method.block,
                &method.attrs,
                FnPathScope::ModuleFns,
            ))
        })
        .flatten()
        .collect()
}

fn merge_recursive_fn_blocks(
    items: &[OrderingItem<'_>],
    helper_call_graph: &[Vec<usize>],
    blocks: &mut AtomicItemBlocks,
) {
    let private_fn_graph: Vec<_> = items
        .iter()
        .zip(helper_call_graph)
        .map(|(item, callee_item_indices)| {
            if item.ast.is_fn() && item.metadata.visibility == Some(VisibilityClass::Private) {
                callee_item_indices.clone()
            } else {
                Vec::new()
            }
        })
        .collect();

    let mut reverse_call_graph = vec![Vec::new(); private_fn_graph.len()];
    for (caller_item_index, callee_item_indices) in private_fn_graph.iter().enumerate() {
        for &callee_item_index in callee_item_indices {
            if let Some(callers) = reverse_call_graph.get_mut(callee_item_index) {
                callers.push(caller_item_index);
            }
        }
    }

    let mut visited = vec![false; private_fn_graph.len()];
    let mut finish_order = Vec::new();
    for item_index in 0..private_fn_graph.len() {
        record_graph_finish_order(item_index, &private_fn_graph, &mut visited, &mut finish_order);
    }

    visited.fill(false);

    for item_index in finish_order.into_iter().rev() {
        if visited.get(item_index).copied().unwrap_or(false) {
            continue;
        }

        let mut recursive_item_indices = Vec::new();
        collect_reachable_component(
            item_index,
            &reverse_call_graph,
            &mut visited,
            &mut recursive_item_indices,
        );

        if recursive_item_indices.len() > 1 {
            recursive_item_indices.sort_unstable();
            blocks.merge(recursive_item_indices, OrderingConstraintKind::RecursiveFnAdjacency);
        }
    }
}

fn record_graph_finish_order(
    start_item_index: usize,
    graph: &[Vec<usize>],
    visited: &mut [bool],
    finish_order: &mut Vec<usize>,
) {
    let mut stack = vec![(start_item_index, false)];
    while let Some((item_index, successors_visited)) = stack.pop() {
        if successors_visited {
            finish_order.push(item_index);
            continue;
        }

        if visited.get(item_index).copied().unwrap_or(false) {
            continue;
        }

        let Some(visited_node) = visited.get_mut(item_index) else {
            continue;
        };

        *visited_node = true;
        stack.push((item_index, true));

        if let Some(successor_items) = graph.get(item_index) {
            for &successor_item_index in successor_items.iter().rev() {
                stack.push((successor_item_index, false));
            }
        }
    }
}

fn collect_reachable_component(
    start_item_index: usize,
    graph: &[Vec<usize>],
    visited: &mut [bool],
    component_items: &mut Vec<usize>,
) {
    let mut stack = vec![start_item_index];
    while let Some(item_index) = stack.pop() {
        if visited.get(item_index).copied().unwrap_or(false) {
            continue;
        }
        let Some(visited_node) = visited.get_mut(item_index) else {
            continue;
        };
        *visited_node = true;
        component_items.push(item_index);
        if let Some(successor_items) = graph.get(item_index) {
            stack.extend(successor_items.iter().rev().copied());
        }
    }
}

fn add_section_group_visibility_constraints(
    items: &[OrderingItem<'_>],
    blocks: &AtomicItemBlocks,
    precedence_graph: &mut BlockPrecedenceGraph,
) {
    // Prefer normal placement of later item groups before displacing an earlier group around macros.
    let mut ranked_blocks: Vec<_> = blocks.members.iter().enumerate().collect();
    ranked_blocks.sort_by_key(|(_, members)| {
        std::cmp::Reverse(
            members
                .first()
                .and_then(|&index| items.get(index))
                .map(item_group_sort_key),
        )
    });
    for (before_block_index, members) in ranked_blocks {
        let Some(item) = members.first().and_then(|&index| items.get(index)) else {
            continue;
        };

        for (after_block_index, following) in blocks.members.iter().enumerate() {
            let Some(other) = following.first().and_then(|&index| items.get(index)) else {
                continue;
            };

            if item.section != other.section {
                if item.section < other.section {
                    precedence_graph.add_precedence(
                        before_block_index,
                        after_block_index,
                        OrderingConstraintKind::GroupOrTestSectionOrder,
                        blocks,
                    );
                }
                continue;
            }

            let module_item_scope = matches!(item.ast, ItemAst::ModuleItem(_));
            let before_rank = item_group_sort_key(item);
            let after_rank = item_group_sort_key(other);

            if module_item_scope && before_rank < after_rank {
                precedence_graph.add_precedence(
                    before_block_index,
                    after_block_index,
                    OrderingConstraintKind::GroupOrTestSectionOrder,
                    blocks,
                );
            } else if item.ast.allows_visibility_order_with(other.ast)
                && (!module_item_scope || (before_rank == after_rank && item.metadata.group == other.metadata.group))
                && let (Some(before_visibility), Some(after_visibility)) =
                    (item.metadata.visibility, other.metadata.visibility)
                && before_visibility < after_visibility
            {
                precedence_graph.add_precedence(
                    before_block_index,
                    after_block_index,
                    OrderingConstraintKind::VisibilityOrder,
                    blocks,
                );
            }
        }
    }
}

fn item_group_sort_key(item: &OrderingItem<'_>) -> (usize, bool) {
    let test_module = matches!(item.ast, ItemAst::ModuleItem(Item::Mod(module))
        if module.content.is_none() && ast::is_test_module_declaration(module));
    (item_group_priority(item.metadata.group), test_module)
}

const fn item_group_priority(group: ItemGroup) -> usize {
    match group {
        ItemGroup::ExternCrate => 0,
        ItemGroup::Use => 1,
        ItemGroup::Modules => 2,
        ItemGroup::GlobalAsm => 3,
        ItemGroup::Constants => 4,
        ItemGroup::Aliases => 5,
        ItemGroup::Items => 6,
    }
}

fn add_caller_before_helper_constraints(
    items: &[OrderingItem<'_>],
    blocks: &AtomicItemBlocks,
    helper_call_graph: &[Vec<usize>],
    precedence_graph: &mut BlockPrecedenceGraph,
) {
    for (helper_block_index, members) in blocks.members.iter().enumerate() {
        let Some(item) = members.first().and_then(|&index| items.get(index)) else {
            continue;
        };
        if !item.ast.is_fn() || item.metadata.visibility != Some(VisibilityClass::Private) {
            continue;
        }

        let caller_blocks: Vec<_> = blocks
            .members
            .iter()
            .enumerate()
            .filter(|(caller_block_index, caller_members)| {
                *caller_block_index != helper_block_index
                    && caller_members.iter().any(|&index| {
                        helper_call_graph.get(index).is_some_and(|callee_item_indices| {
                            callee_item_indices
                                .iter()
                                .any(|callee_item_index| members.contains(callee_item_index))
                        })
                    })
            })
            .map(|(caller_block_index, _)| caller_block_index)
            .collect();

        let prefer_public = members.len() > 1 || caller_blocks.len() > 1;

        let public_caller_blocks: Vec<_> = caller_blocks
            .iter()
            .copied()
            .filter(|&caller_block_index| {
                blocks
                    .members
                    .get(caller_block_index)
                    .and_then(|members| members.first())
                    .and_then(|&index| items.get(index))
                    .is_some_and(|item| item.metadata.visibility == Some(VisibilityClass::Public))
            })
            .collect();

        let eligible_caller_blocks = if prefer_public && !public_caller_blocks.is_empty() {
            &public_caller_blocks
        } else {
            &caller_blocks
        };

        let earliest_eligible_caller_block = eligible_caller_blocks
            .iter()
            .copied()
            .min_by_key(|&caller_block_index| {
                blocks
                    .members
                    .get(caller_block_index)
                    .and_then(|members| members.iter().min())
                    .copied()
            });

        if let Some(earliest_eligible_caller_block) = earliest_eligible_caller_block {
            precedence_graph.add_precedence(
                earliest_eligible_caller_block,
                helper_block_index,
                OrderingConstraintKind::CallerBeforeHelper,
                blocks,
            );
        }
    }
}
