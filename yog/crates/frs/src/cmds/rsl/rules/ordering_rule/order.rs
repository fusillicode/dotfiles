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

use super::Arrangement;
use super::OrderingRuleReason;
use super::Preference;
use super::ScopeItem;
use super::SyntaxItem;
use super::ast;
use super::calls;
use super::calls::CallScope;
use crate::cmds::rsl::ast::ItemGroup;
use crate::cmds::rsl::ast::VisibilityClass;

pub(super) fn arrange(items: &mut [ScopeItem<'_>]) -> Arrangement {
    let mut blocks = Blocks::new(items);
    if let Err(message) = type_blocks(items, &mut blocks) {
        return Arrangement::Conflict {
            message,
            reasons: blocks.reasons,
        };
    }

    let targets = function_targets(items);
    let calls = call_graph(items, &targets);
    recursive_blocks(items, &calls, &mut blocks);
    for members in &blocks.members {
        if let Some(&identity) = members.first() {
            for &member in members {
                if let Some(item) = items.get_mut(member) {
                    item.details.block = identity;
                }
            }
        }
    }

    let mut relations = Relations::new(&blocks);
    let deferred = macro_relations(items, &blocks, &mut relations);
    section_relations(items, &blocks, &deferred, &mut relations);
    caller_relations(items, &blocks, &calls, &mut relations);

    let sequence = relations.sequence(&blocks);
    let sequence = match sequence {
        Ok(sequence) => sequence,
        Err(remaining) => {
            let cycle: Vec<_> = relations
                .cycle(&remaining)
                .iter()
                .filter_map(|&block| {
                    blocks
                        .members
                        .get(block)
                        .and_then(|members| members.first())
                        .and_then(|&index| items.get(index))
                })
                .map(|item| format!("{}@{}", item.details.label, item.details.range.compact()))
                .collect();
            return Arrangement::Conflict {
                message: format!("cycle [{}]", cycle.join(" -> ")),
                reasons: blocks.reasons.into_iter().chain(relations.reasons).collect(),
            };
        }
    };

    let reasons = std::mem::take(&mut blocks.reasons)
        .into_iter()
        .chain(relations.reasons)
        .collect();

    let required = blocks.expand(&sequence);
    let original: Vec<_> = (0..items.len()).collect();

    Arrangement::Ordered {
        original,
        required,
        reasons,
    }
}

struct Blocks {
    members: Vec<Vec<usize>>,
    reasons: Vec<OrderingRuleReason>,
}

impl Blocks {
    fn new(items: &[ScopeItem<'_>]) -> Self {
        Self {
            members: (0..items.len()).map(|index| vec![index]).collect(),
            reasons: Vec::new(),
        }
    }

    fn containing(&self, item: usize) -> Option<usize> {
        self.members.iter().position(|members| members.contains(&item))
    }

    fn merge(&mut self, members: Vec<usize>, preference: Preference) {
        for pair in members.windows(2) {
            if let [before, after] = pair {
                self.reasons.push(OrderingRuleReason {
                    before: *before,
                    after: *after,
                    preference,
                });
            }
        }
        self.members
            .retain(|block| !block.iter().any(|item| members.contains(item)));
        self.members.push(members);
        self.members.sort_by_key(|block| block.iter().min().copied());
    }

    fn expand(&self, sequence: &[usize]) -> Vec<usize> {
        sequence
            .iter()
            .filter_map(|&block| self.members.get(block))
            .flatten()
            .copied()
            .collect()
    }
}

struct Relations {
    edges: Vec<Vec<usize>>,
    reasons: Vec<OrderingRuleReason>,
}

impl Relations {
    fn new(blocks: &Blocks) -> Self {
        Self {
            edges: vec![Vec::new(); blocks.members.len()],
            reasons: Vec::new(),
        }
    }

    fn add(&mut self, before: usize, after: usize, preference: Preference, blocks: &Blocks) {
        if before == after {
            return;
        }

        if let Some(edges) = self.edges.get_mut(before)
            && !edges.contains(&after)
        {
            edges.push(after);
            if let (Some(before), Some(after)) = (
                blocks.members.get(before).and_then(|m| m.last()),
                blocks.members.get(after).and_then(|m| m.first()),
            ) {
                self.reasons.push(OrderingRuleReason {
                    before: *before,
                    after: *after,
                    preference,
                });
            }
        }
    }

    fn sequence(&self, blocks: &Blocks) -> Result<Vec<usize>, Vec<usize>> {
        let mut incoming = vec![0_usize; self.edges.len()];
        for &target in self.edges.iter().flatten() {
            if let Some(count) = incoming.get_mut(target) {
                *count = count.saturating_add(1);
            }
        }

        let source_key = |block: usize| {
            blocks
                .members
                .get(block)
                .and_then(|members| members.iter().min())
                .copied()
        };

        let mut ready: BTreeSet<_> = incoming
            .iter()
            .enumerate()
            .filter(|(_, count)| **count == 0)
            .map(|(block, _)| (source_key(block), block))
            .collect();

        let mut result = Vec::new();
        while let Some((_, next)) = ready.pop_first() {
            result.push(next);
            for &target in self.edges.get(next).into_iter().flatten() {
                if let Some(count) = incoming.get_mut(target) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.insert((source_key(target), target));
                    }
                }
            }
        }

        if result.len() == self.edges.len() {
            return Ok(result);
        }

        Err(incoming
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(block, _)| block)
            .collect())
    }

    fn cycle(&self, remaining: &[usize]) -> Vec<usize> {
        for &start in remaining {
            let mut pending = VecDeque::from([vec![start]]);
            let mut visited = HashSet::from([start]);
            while let Some(path) = pending.pop_front() {
                let Some(&last) = path.last() else {
                    continue;
                };

                for &next in self.edges.get(last).into_iter().flatten() {
                    if !remaining.contains(&next) {
                        continue;
                    }
                    let mut extended = path.clone();
                    extended.push(next);

                    if next == start {
                        return extended;
                    }

                    if visited.insert(next) {
                        pending.push_back(extended);
                    }
                }
            }
        }

        Vec::new()
    }
}

fn type_blocks(items: &[ScopeItem<'_>], blocks: &mut Blocks) -> Result<(), String> {
    let mut declarations: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        if let SyntaxItem::Module(item) = item.syntax
            && let Some(name) = ast::type_definition_name(item)
        {
            declarations.entry(name).or_default().push(index);
        }
    }

    let mut implementations: HashMap<usize, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let SyntaxItem::Module(Item::Impl(item)) = item.syntax else {
            continue;
        };
        let Some(name) = ast::impl_target_name(item) else {
            continue;
        };
        let Some(candidates) = declarations.get(&name) else {
            continue;
        };

        let declaration = if candidates.len() == 1 {
            candidates.first().copied()
        } else {
            candidates
                .iter()
                .rev()
                .copied()
                .find(|&declaration| declaration < index)
        };

        let Some(declaration) = declaration else {
            return Err(format!(
                "impl {name} has multiple declarations and no preceding declaration"
            ));
        };

        let (inherent, traits) = implementations.entry(declaration).or_default();

        if item.trait_.is_none() {
            inherent.push(index);
        } else {
            traits.push(index);
        }
    }

    let mut implementations: Vec<_> = implementations.into_iter().collect();
    implementations.sort_by_key(|(declaration, _)| *declaration);

    for (declaration, (inherent, traits)) in implementations {
        let members = std::iter::once(declaration).chain(inherent).chain(traits).collect();
        blocks.merge(members, Preference::TypeBlock);
    }

    Ok(())
}

fn function_targets(items: &[ScopeItem<'_>]) -> HashMap<String, usize> {
    let mut targets = HashMap::new();
    let mut ambiguous = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(name) = item.syntax.function_name().map(ToString::to_string)
            && targets.insert(name.clone(), index).is_some()
        {
            ambiguous.insert(name);
        }
    }
    for name in ambiguous {
        targets.remove(&name);
    }
    targets
}

fn call_graph(items: &[ScopeItem<'_>], targets: &HashMap<String, usize>) -> Vec<Vec<usize>> {
    items
        .iter()
        .map(|item| {
            let mut called: Vec<_> = item_calls(item)
                .iter()
                .filter_map(|name| targets.get(name).copied())
                .filter(|&target| {
                    items.get(target).is_some_and(|target| {
                        target.section == item.section && target.details.visibility == Some(VisibilityClass::Private)
                    })
                })
                .collect();
            called.sort_unstable();
            called.dedup();
            called
        })
        .collect()
}

fn item_calls(item: &ScopeItem<'_>) -> Vec<String> {
    match item.syntax {
        SyntaxItem::Module(Item::Fn(item)) => {
            return calls::direct_calls(&item.block, &item.attrs, CallScope::Module);
        }
        SyntaxItem::Associated(ImplItem::Fn(item)) => {
            return calls::direct_calls(&item.block, &item.attrs, CallScope::Associated);
        }
        SyntaxItem::Module(_) | SyntaxItem::Associated(_) => {}
    }
    let SyntaxItem::Module(Item::Impl(item)) = item.syntax else {
        return Vec::new();
    };

    item.items
        .iter()
        .filter_map(|item| {
            let ImplItem::Fn(method) = item else {
                return None;
            };
            Some(calls::direct_calls(&method.block, &method.attrs, CallScope::Module))
        })
        .flatten()
        .collect()
}

fn recursive_blocks(items: &[ScopeItem<'_>], calls: &[Vec<usize>], blocks: &mut Blocks) {
    let graph: Vec<_> = items
        .iter()
        .zip(calls)
        .map(|(item, called)| {
            if item.syntax.is_function() && item.details.visibility == Some(VisibilityClass::Private) {
                called.clone()
            } else {
                Vec::new()
            }
        })
        .collect();

    let mut reverse = vec![Vec::new(); graph.len()];
    for (caller, edges) in graph.iter().enumerate() {
        for &target in edges {
            if let Some(callers) = reverse.get_mut(target) {
                callers.push(caller);
            }
        }
    }

    let mut visited = vec![false; graph.len()];
    let mut order = Vec::new();
    for node in 0..graph.len() {
        visit_graph(node, &graph, &mut visited, &mut order);
    }

    visited.fill(false);

    for node in order.into_iter().rev() {
        if visited.get(node).copied().unwrap_or(false) {
            continue;
        }

        let mut component = Vec::new();
        collect_graph_component(node, &reverse, &mut visited, &mut component);

        if component.len() > 1 {
            component.sort_unstable();
            blocks.merge(component, Preference::RecursiveBlock);
        }
    }
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

#[derive(Default)]
struct MacroUses {
    collected: HashSet<String>,
    shadowed: HashSet<String>,
}

impl<'ast> Visit<'ast> for MacroUses {
    fn visit_macro(&mut self, mac: &'ast Macro) {
        if mac.path.leading_colon.is_none()
            && mac.path.segments.len() == 1
            && let Some(segment) = mac.path.segments.first()
        {
            let name = segment.ident.to_string();
            if !self.shadowed.contains(&name) {
                self.collected.insert(name);
            }
        }
    }

    fn visit_item_macro(&mut self, item: &'ast ItemMacro) {
        if item.mac.path.is_ident("macro_rules")
            && let Some(name) = &item.ident
        {
            self.shadowed.insert(name.to_string());
        } else {
            visit::visit_item_macro(self, item);
        }
    }

    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        let inherited = self.shadowed.clone();
        visit::visit_item_mod(self, module);
        self.shadowed = inherited;
    }

    fn visit_block(&mut self, block: &'ast Block) {
        let inherited = self.shadowed.clone();
        visit::visit_block(self, block);
        self.shadowed = inherited;
    }
}

fn macro_relations(items: &[ScopeItem<'_>], blocks: &Blocks, relations: &mut Relations) -> HashSet<usize> {
    let definitions: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let SyntaxItem::Module(Item::Macro(mac)) = item.syntax else {
                return None;
            };
            if !mac.mac.path.is_ident("macro_rules") {
                return None;
            }
            mac.ident.as_ref().map(|name| (index, name))
        })
        .collect();
    let mut deferred = HashSet::new();
    if definitions.is_empty() {
        return deferred;
    }

    let consumers: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let SyntaxItem::Module(Item::Mod(module)) = item.syntax else {
                return None;
            };
            module.content.as_ref()?;
            let mut uses = MacroUses::default();
            uses.visit_item_mod(module);
            Some((index, uses.collected))
        })
        .collect();

    for &(definition, name) in &definitions {
        let name_key = name.to_string();
        for (consumer, uses) in &consumers {
            if !uses.contains(&name_key) {
                continue;
            }
            let consumer = *consumer;
            if let (Some(before), Some(after)) = (blocks.containing(definition), blocks.containing(consumer)) {
                let preceding_definition = definitions
                    .iter()
                    .any(|&(index, previous)| index < consumer && previous == name);
                if definition > consumer && preceding_definition {
                    // Moving this later definition before the module would change its lexical binding.
                    relations.add(after, before, Preference::MacroScope, blocks);
                } else {
                    relations.add(before, after, Preference::MacroScope, blocks);
                }
                deferred.insert(after);
            }
        }
    }

    deferred
}

fn section_relations(items: &[ScopeItem<'_>], blocks: &Blocks, deferred: &HashSet<usize>, relations: &mut Relations) {
    for (before, members) in blocks.members.iter().enumerate() {
        let Some(item) = members.first().and_then(|&index| items.get(index)) else {
            continue;
        };

        for (after, following) in blocks.members.iter().enumerate() {
            let Some(other) = following.first().and_then(|&index| items.get(index)) else {
                continue;
            };

            if item.section != other.section {
                if item.section < other.section {
                    relations.add(before, after, Preference::Group, blocks);
                }
                continue;
            }

            let module_scope = matches!(item.syntax, SyntaxItem::Module(_));
            let first_rank = rank(item, deferred.contains(&before));
            let second_rank = rank(other, deferred.contains(&after));

            if module_scope && first_rank < second_rank {
                relations.add(before, after, Preference::Group, blocks);
            } else if (!module_scope || (first_rank == second_rank && item.details.group == other.details.group))
                // Rustfmt groups and sorts imports by path, independently of visibility.
                && item.details.group != ItemGroup::Use
                && let (Some(first), Some(second)) = (item.details.visibility, other.details.visibility)
                && first < second
            {
                relations.add(before, after, Preference::Visibility, blocks);
            }
        }
    }
}

fn rank(item: &ScopeItem<'_>, deferred: bool) -> (usize, bool) {
    if deferred {
        return (group_rank(ItemGroup::Items), false);
    }
    let test_module = matches!(item.syntax, SyntaxItem::Module(module) if ast::is_test_module(module));
    (group_rank(item.details.group), test_module)
}

const fn group_rank(group: ItemGroup) -> usize {
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

fn caller_relations(items: &[ScopeItem<'_>], blocks: &Blocks, calls: &[Vec<usize>], relations: &mut Relations) {
    for (helper, members) in blocks.members.iter().enumerate() {
        let Some(item) = members.first().and_then(|&index| items.get(index)) else {
            continue;
        };
        if !item.syntax.is_function() || item.details.visibility != Some(VisibilityClass::Private) {
            continue;
        }

        let callers: Vec<_> = blocks
            .members
            .iter()
            .enumerate()
            .filter(|(caller, caller_members)| {
                *caller != helper
                    && caller_members.iter().any(|&index| {
                        calls
                            .get(index)
                            .is_some_and(|called| called.iter().any(|target| members.contains(target)))
                    })
            })
            .map(|(caller, _)| caller)
            .collect();

        let prefer_public = members.len() > 1 || callers.len() > 1;

        let public: Vec<_> = callers
            .iter()
            .copied()
            .filter(|&caller| {
                blocks
                    .members
                    .get(caller)
                    .and_then(|members| members.first())
                    .and_then(|&index| items.get(index))
                    .is_some_and(|item| item.details.visibility == Some(VisibilityClass::Public))
            })
            .collect();

        let eligible = if prefer_public && !public.is_empty() {
            &public
        } else {
            &callers
        };

        let anchor = eligible.iter().copied().min_by_key(|&caller| {
            blocks
                .members
                .get(caller)
                .and_then(|members| members.iter().min())
                .copied()
        });

        if let Some(anchor) = anchor {
            relations.add(anchor, helper, Preference::Caller, blocks);
        }
    }
}
