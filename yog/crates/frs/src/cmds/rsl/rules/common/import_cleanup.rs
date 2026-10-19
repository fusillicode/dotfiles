//! Apply import removals selected by the call qualification plan.

use std::collections::HashSet;
use std::ops::Range;

use syn::Item;
use syn::UseGroup;
use syn::UseTree;
use syn::Visibility;
use syn::punctuated::Iter;
use syn::spanned::Spanned;

use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::common;
use crate::cmds::rsl::rules::common::import_resolution;
use crate::cmds::rsl::source::SourceEditor;

pub(in crate::cmds::rsl::rules) fn removable_imports(
    ctx: &FileContext<'_>,
    items: &[Item],
    editor: &SourceEditor<'_>,
) -> HashSet<Range<usize>> {
    items
        .iter()
        .filter_map(|item| {
            let Item::Use(import) = item else {
                return None;
            };
            let text = ctx.source.get(editor.span_range(import.span())).unwrap_or_default();
            (matches!(import.vis, Visibility::Inherited)
                && !common::has_conditional_attributes(&import.attrs)
                && !text.contains("//")
                && !text.contains("/*"))
            .then(|| import_resolution::use_bindings(&import.tree, &[]).bindings)
        })
        .flatten()
        .map(|binding| editor.span_range(binding.span))
        .collect()
}

pub(in crate::cmds::rsl::rules) fn plan_import_removals(
    items: &[Item],
    removed: &HashSet<Range<usize>>,
    plan: &mut RulePlan<'_>,
) -> rootcause::Result<()> {
    if removed.is_empty() {
        return Ok(());
    }
    for item in items {
        let Item::Use(import) = item else {
            continue;
        };
        let ranges =
            removed_ranges(&import.tree, removed, &plan.edits).into_ranges(plan.edits.span_range(import.span()));
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

fn removed_ranges(mut tree: &UseTree, unused: &HashSet<Range<usize>>, editor: &SourceEditor<'_>) -> TreeEdits {
    let mut groups: Vec<PendingGroup<'_>> = Vec::new();

    'visit: loop {
        let mut edits = match tree {
            UseTree::Name(name) => {
                if unused.contains(&editor.span_range(name.span())) {
                    TreeEdits::Remove
                } else {
                    TreeEdits::Keep(Vec::new())
                }
            }
            UseTree::Rename(rename) => {
                if unused.contains(&editor.span_range(rename.span())) {
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

                TreeEdits::Keep(Vec::new())
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
