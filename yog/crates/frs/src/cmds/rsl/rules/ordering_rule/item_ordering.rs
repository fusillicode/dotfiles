//! Atomic blocks and precedence relationships, with stable source-order ties.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use syn::Block;
use syn::ImplItem;
use syn::Item;
use syn::ItemMacro;
use syn::ItemMod;
use syn::Macro;
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
    let deferred_module_blocks = add_macro_binding_constraints(items, &blocks, &mut precedence_graph);
    add_section_group_visibility_constraints(items, &blocks, &deferred_module_blocks, &mut precedence_graph);
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

struct BlockPrecedenceGraph {
    successor_blocks: Vec<Vec<usize>>,
    constraints: Vec<OrderingConstraint>,
}

impl BlockPrecedenceGraph {
    fn new(blocks: &AtomicItemBlocks) -> Self {
        Self {
            successor_blocks: vec![Vec::new(); blocks.members.len()],
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

        if let Some(successor_blocks) = self.successor_blocks.get_mut(before_block_index)
            && !successor_blocks.contains(&after_block_index)
        {
            successor_blocks.push(after_block_index);
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

    fn stable_topological_order(&self, blocks: &AtomicItemBlocks) -> Result<Vec<usize>, Vec<usize>> {
        let mut incoming_edge_counts = vec![0_usize; self.successor_blocks.len()];
        for &successor_block in self.successor_blocks.iter().flatten() {
            if let Some(count) = incoming_edge_counts.get_mut(successor_block) {
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
            for &successor_block in self.successor_blocks.get(next_block_index).into_iter().flatten() {
                if let Some(count) = incoming_edge_counts.get_mut(successor_block) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready_blocks.insert((earliest_source_item_index(successor_block), successor_block));
                    }
                }
            }
        }

        if block_order.len() == self.successor_blocks.len() {
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

                for &next_block_index in self.successor_blocks.get(last_block_index).into_iter().flatten() {
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

#[derive(Default)]
struct UnshadowedMacroNameCollector {
    referenced_names: HashSet<String>,
    shadowed_names: HashSet<String>,
}

impl<'ast> Visit<'ast> for UnshadowedMacroNameCollector {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        if mac.path.leading_colon.is_none()
            && mac.path.segments.len() == 1
            && let Some(segment) = mac.path.segments.first()
        {
            let name = segment.ident.to_string();
            if !self.shadowed_names.contains(&name) {
                self.referenced_names.insert(name);
            }
        }
    }

    fn visit_item_macro(&mut self, item: &'ast ItemMacro) {
        if item.mac.path.is_ident("macro_rules")
            && let Some(name) = &item.ident
        {
            self.shadowed_names.insert(name.to_string());
        } else {
            visit::visit_item_macro(self, item);
        }
    }

    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        let inherited_shadowed_names = self.shadowed_names.clone();
        visit::visit_item_mod(self, module);
        self.shadowed_names = inherited_shadowed_names;
    }

    fn visit_block(&mut self, block: &'ast Block) {
        let inherited_shadowed_names = self.shadowed_names.clone();
        visit::visit_block(self, block);
        self.shadowed_names = inherited_shadowed_names;
    }
}

fn add_macro_binding_constraints(
    items: &[OrderingItem<'_>],
    blocks: &AtomicItemBlocks,
    precedence_graph: &mut BlockPrecedenceGraph,
) -> HashSet<usize> {
    let macro_definitions: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let ItemAst::ModuleItem(Item::Macro(mac)) = item.ast else {
                return None;
            };
            if !mac.mac.path.is_ident("macro_rules") {
                return None;
            }
            mac.ident.as_ref().map(|name| (index, name))
        })
        .collect();
    let mut deferred_module_blocks = HashSet::new();
    if macro_definitions.is_empty() {
        return deferred_module_blocks;
    }

    let consumer_modules: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let ItemAst::ModuleItem(Item::Mod(module)) = item.ast else {
                return None;
            };
            module.content.as_ref()?;
            let mut macro_references = UnshadowedMacroNameCollector::default();
            macro_references.visit_item_mod(module);
            Some((index, macro_references.referenced_names))
        })
        .collect();

    for &(macro_definition_index, name) in &macro_definitions {
        let name_key = name.to_string();
        for (consumer_module_index, macro_references) in &consumer_modules {
            if !macro_references.contains(&name_key) {
                continue;
            }
            let consumer_module_index = *consumer_module_index;
            if let (Some(definition_block_index), Some(consumer_block_index)) = (
                blocks.block_index_containing_item(macro_definition_index),
                blocks.block_index_containing_item(consumer_module_index),
            ) {
                let has_preceding_definition = macro_definitions
                    .iter()
                    .any(|&(index, previous)| index < consumer_module_index && previous == name);
                if macro_definition_index > consumer_module_index && has_preceding_definition {
                    // Moving this later definition before the module would change its lexical binding.
                    precedence_graph.add_precedence(
                        consumer_block_index,
                        definition_block_index,
                        OrderingConstraintKind::MacroBindingPreservation,
                        blocks,
                    );
                } else {
                    precedence_graph.add_precedence(
                        definition_block_index,
                        consumer_block_index,
                        OrderingConstraintKind::MacroBindingPreservation,
                        blocks,
                    );
                }
                deferred_module_blocks.insert(consumer_block_index);
            }
        }
    }

    deferred_module_blocks
}

fn add_section_group_visibility_constraints(
    items: &[OrderingItem<'_>],
    blocks: &AtomicItemBlocks,
    deferred_module_blocks: &HashSet<usize>,
    precedence_graph: &mut BlockPrecedenceGraph,
) {
    for (before_block_index, members) in blocks.members.iter().enumerate() {
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
            let before_rank = item_group_sort_key(item, deferred_module_blocks.contains(&before_block_index));
            let after_rank = item_group_sort_key(other, deferred_module_blocks.contains(&after_block_index));

            if module_item_scope && before_rank < after_rank {
                precedence_graph.add_precedence(
                    before_block_index,
                    after_block_index,
                    OrderingConstraintKind::GroupOrTestSectionOrder,
                    blocks,
                );
            } else if (!module_item_scope || (before_rank == after_rank && item.metadata.group == other.metadata.group))
                // Rustfmt groups and sorts imports by path, independently of visibility.
                && item.metadata.group != ItemGroup::Use
                && let (Some(before_visibility), Some(after_visibility)) = (item.metadata.visibility, other.metadata.visibility)
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

fn item_group_sort_key(item: &OrderingItem<'_>, is_deferred_module: bool) -> (usize, bool) {
    if is_deferred_module {
        return (item_group_priority(ItemGroup::Items), false);
    }
    let test_module = matches!(item.ast, ItemAst::ModuleItem(module) if ast::is_test_module(module));
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
