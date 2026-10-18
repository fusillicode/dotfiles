//! Remove only private function imports whose references were all rewritten.

use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;

use proc_macro2::Span;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use syn::Attribute;
use syn::Item;
use syn::ItemUse;
use syn::Macro;
use syn::Meta;
use syn::Path;
use syn::UseGroup;
use syn::UseTree;
use syn::Visibility;
use syn::punctuated::Iter;
use syn::spanned::Spanned;
use syn::visit::Visit;

use super::comments_reason;
use super::import_resolution::use_bindings;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

struct UnusedFunctionImportChange<'source> {
    span: Span,
    import: &'source str,
}

impl RuleChange for UnusedFunctionImportChange<'_> {
    fn span(&self) -> Span {
        self.span
    }

    fn description(&self) -> String {
        format!("remove unused function import `{}`", self.import)
    }
}

pub(in crate::cmds::rsl::rules) fn plan_unused_fn_import_removals(
    ctx: &FileContext<'_>,
    items: &[Item],
    changed: &[Span],
    plan: &mut RulePlan<'_>,
) -> rootcause::Result<()> {
    if changed.is_empty() {
        return Ok(());
    }

    let mut references = References { counts: HashMap::new() };
    references.visit_file(ctx.file);

    let mut rewritten: HashMap<&str, usize> = HashMap::new();

    for span in changed {
        let name = ctx.source.get(plan.edits.span_range(*span)).unwrap_or_default();
        let count = rewritten.entry(name).or_default();
        *count = count.saturating_add(1);
    }

    let unused: HashSet<_> = rewritten
        .into_iter()
        .filter(|(name, count)| references.counts.get(*name) == Some(count))
        .map(|(name, _)| name)
        .collect();

    for item in items {
        let Item::Use(item_use) = item else {
            continue;
        };

        if !matches!(item_use.vis, Visibility::Inherited) || !item_use.attrs.is_empty() {
            continue;
        }

        let bindings = use_bindings(&item_use.tree, &[]);

        if !bindings
            .bindings
            .iter()
            .any(|binding| unused.contains(binding.name.as_str()))
        {
            continue;
        }

        let text = ctx
            .source
            .get(plan.edits.span_range(item_use.span()))
            .unwrap_or_default();

        if text.contains("//") || text.contains("/*") {
            let change = UnusedFunctionImportChange {
                span: item_use.span(),
                import: text,
            };
            plan.record_blocked(&change, comments_reason());
            continue;
        }

        let ranges =
            removed_ranges(&item_use.tree, &unused, &plan.edits).into_ranges(plan.edits.span_range(item_use.span()));

        for range in ranges {
            plan.edits.remove(range)?;
        }
    }

    Ok(())
}

enum TreeEdits {
    Remove,
    Keep(Vec<Range<usize>>),
}

impl TreeEdits {
    fn into_ranges(self, whole_import: Range<usize>) -> Vec<Range<usize>> {
        match self {
            Self::Remove => vec![whole_import],
            Self::Keep(ranges) => ranges,
        }
    }
}

struct PendingGroup<'ast> {
    group: &'ast UseGroup,
    remaining: Iter<'ast, UseTree>,
    edits: Vec<TreeEdits>,
}

fn removed_ranges(mut tree: &UseTree, unused: &HashSet<&str>, editor: &SourceEditor<'_>) -> TreeEdits {
    let mut groups: Vec<PendingGroup<'_>> = Vec::new();

    'visit: loop {
        let mut edits = match tree {
            UseTree::Name(name) => {
                if unused.contains(name.ident.to_string().as_str()) {
                    TreeEdits::Remove
                } else {
                    TreeEdits::Keep(Vec::new())
                }
            }
            UseTree::Rename(rename) => {
                if unused.contains(rename.rename.to_string().as_str()) {
                    TreeEdits::Remove
                } else {
                    TreeEdits::Keep(Vec::new())
                }
            }
            UseTree::Glob(_) => TreeEdits::Keep(Vec::new()),
            UseTree::Path(path) => {
                tree = &path.tree;
                continue;
            }
            UseTree::Group(group) => {
                let mut remaining = group.items.iter();

                if let Some(child) = remaining.next() {
                    groups.push(PendingGroup {
                        group,
                        remaining,
                        edits: Vec::new(),
                    });
                    tree = child;
                    continue;
                }

                TreeEdits::Remove
            }
        };

        while let Some(mut group) = groups.pop() {
            group.edits.push(edits);

            if let Some(child) = group.remaining.next() {
                tree = child;
                groups.push(group);
                continue 'visit;
            }

            edits = group_removed_ranges(group.group, group.edits, editor);
        }

        return edits;
    }
}

fn group_removed_ranges(group: &UseGroup, edits: Vec<TreeEdits>, editor: &SourceEditor<'_>) -> TreeEdits {
    let children: Vec<_> = group.items.pairs().zip(edits).collect();

    if children.iter().all(|(_, edits)| matches!(edits, TreeEdits::Remove)) {
        return TreeEdits::Remove;
    }

    let mut result = Vec::new();

    for (index, (pair, edits)) in children.iter().enumerate() {
        if let TreeEdits::Keep(ranges) = edits {
            result.extend(ranges.iter().cloned());
            continue;
        }

        let range = editor.span_range(pair.value().span());

        if let Some(comma) = pair.punct() {
            result.push(range.start..editor.offset(comma.span.end()));
        } else if let Some(comma) = index
            .checked_sub(1)
            .and_then(|index| children.get(index))
            .and_then(|(pair, _)| pair.punct())
        {
            result.push(editor.offset(comma.span.start())..range.end);
        } else {
            result.push(range);
        }
    }

    result.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();

    for range in result {
        if let Some(previous) = merged.last_mut()
            && previous.end >= range.start
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }

    TreeEdits::Keep(merged)
}

struct References {
    counts: HashMap<String, usize>,
}

impl References {
    fn add(&mut self, name: String) {
        let count = self.counts.entry(name).or_default();
        *count = count.saturating_add(1);
    }

    fn tokens(&mut self, tokens: TokenStream) {
        let mut pending = vec![tokens.into_iter()];

        while let Some(tokens) = pending.last_mut() {
            match tokens.next() {
                Some(TokenTree::Ident(ident)) => self.add(ident.to_string()),
                Some(TokenTree::Group(group)) => pending.push(group.stream().into_iter()),
                Some(TokenTree::Punct(_) | TokenTree::Literal(_)) => {}
                None => {
                    pending.pop();
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for References {
    fn visit_path(&mut self, path: &'ast Path) {
        for segment in &path.segments {
            self.add(segment.ident.to_string());
        }

        syn::visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        self.tokens(mac.tokens.clone());
        syn::visit::visit_macro(self, mac);
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.visit_path(attribute.path());

        if let Meta::List(meta) = &attribute.meta {
            self.tokens(meta.tokens.clone());
        }
    }

    fn visit_item_use(&mut self, _item: &'ast ItemUse) {}
}
