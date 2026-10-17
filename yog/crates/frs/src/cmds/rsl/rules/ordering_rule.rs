//! Deterministic item ordering, with one complete arrangement per affected scope.
//!
//! ```text
//! extern_crate -> use -> foreign_mod/mod -> #[cfg(test)] mod tests;
//! -> global_asm -> const/static -> ty_alias -> macros/types/traits/impls/functions
//! -> inline #[cfg(test)] modules (any name)
//! ```
//!
//! Keep contiguous blocks: `type -> inherent impls -> trait impls`, and mutually recursive private helpers.
//! Within each group or inherent impl: `pub -> pub(crate) -> restricted -> private`.
//! Imports, external module declarations, and associated items other than methods follow the formatter.
//! Test-only files (`tests.rs`, `test_*.rs`, `*_test.rs`, `*_tests.rs`) and inline `#[cfg(test)]`
//! modules inherit `tests -> helpers`; production scopes keep test functions last.
//! Test attribute path segments match `(?:^|_)test(?:_|$)|^rstest$`; arguments do not classify functions.
//! Sections override visibility and callers. Calls and recursive blocks stay within their section.
//! Private helpers follow their earliest eligible caller; shared helpers prefer public callers.
//! Duplicate-name impls attach to the last preceding declaration, without cfg evaluation.
//!
//! Preserve source order between macro sources and possible consumers, without resolving macro names.
//! Sources include macro definitions/invocations, imports, and opaque items. Consumers include attributed items,
//! external modules, and items containing macros or opaque syntax. Plain items can cross macro sources.
//! Ordering preferences yield to macro preservation, except inline test modules must be last.
//! Incompatible test-module placement or adjacency emits a conflict without moves.
//!
//! Conflicting scopes are skipped; independent nested scopes can still be fixed.
//!
//! Apply inner scopes first, retaining original-snapshot identities as earlier edits shift positions.
//! Ranges include attributes and attached comments: `9-11` is inclusive; `9:3-11:2` uses one-based columns
//! and an exclusive end. Same-line trailing comments attach backward; contiguous leading comments attach forward.
//! Blank-separated comments and inner docs stay scope-owned; whitespace gaps stay in their slots.

use std::path::Path;

use proc_macro2::Ident;
use proc_macro2::LineColumn;
use proc_macro2::Span;
use syn::Attribute;
use syn::ImplItem;
use syn::Item;
use syn::ItemMod;
use syn::Meta;
use syn::spanned::Spanned;

use super::Rule;
use crate::cmds::rsl::ast;
use crate::cmds::rsl::ast::ItemGroup;
use crate::cmds::rsl::ast::ItemKind;
use crate::cmds::rsl::ast::VisibilityClass;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;
use crate::cmds::rsl::source::item_ranges;
use crate::cmds::rsl::source::item_ranges::SourceRange;

mod fn_references;
mod item_ordering;

pub(super) struct OrderingRule;

impl Rule for OrderingRule {
    fn id(&self) -> &'static str {
        "ordering-rule"
    }

    fn fix(&self, ctx: &FileContext<'_>, editor: &mut SourceEditor<'_>) -> rootcause::Result<()> {
        let test_ordering = TestOrdering::from(ctx.path);
        let mut pending = vec![
            ScopeWork::Fix {
                items: ctx.file.items.iter().map(ItemAst::ModuleItem).collect(),
                scope: OrderingScope {
                    label: "module crate".to_owned(),
                    body_start: LineColumn { line: 1, column: 0 },
                    test_ordering,
                },
            },
            ScopeWork::Discover {
                items: &ctx.file.items,
                name: "crate".to_owned(),
                test_ordering,
            },
        ];

        while let Some(work) = pending.pop() {
            match work {
                ScopeWork::Discover {
                    items,
                    name,
                    test_ordering,
                } => {
                    schedule_nested_scopes(items, &name, test_ordering, &mut pending);
                }
                ScopeWork::Fix { items, scope } => {
                    fix_scope(ctx, items.into_iter(), &scope, editor)?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct ItemOrderingMetadata {
    pub range: SourceRange,
    pub label: String,
    pub kind: Option<ItemKind>,
    pub visibility: Option<VisibilityClass>,
}

impl ItemOrderingMetadata {
    fn group(&self) -> ItemGroup {
        self.kind.map_or(ItemGroup::Items, ItemKind::group)
    }

    fn new(ast: ItemAst<'_>, range: SourceRange) -> Self {
        let (kind, label, visibility) = match ast {
            ItemAst::ModuleItem(item) => {
                let kind = ast::classify_item(item);
                let label = kind.map_or_else(|| "opaque item".to_owned(), |kind| ast::item_label(item, kind));
                (kind, label, ast::item_visibility(item))
            }
            ItemAst::ImplItem(ImplItem::Fn(item)) => (
                Some(ItemKind::Fn),
                format!("fn {}", item.sig.ident),
                Some((&item.vis).into()),
            ),
            ItemAst::ImplItem(ImplItem::Const(item)) => (
                Some(ItemKind::Const),
                format!("const {}", item.ident),
                Some((&item.vis).into()),
            ),
            ItemAst::ImplItem(ImplItem::Type(item)) => (
                Some(ItemKind::TypeAlias),
                format!("type {}", item.ident),
                Some((&item.vis).into()),
            ),
            ItemAst::ImplItem(ImplItem::Macro(_) | ImplItem::Verbatim(_) | _) => (None, "opaque item".to_owned(), None),
        };

        Self {
            range,
            label,
            kind,
            visibility,
        }
    }
}

/// A complete permutation that satisfies every computed ordering constraint.
struct ItemOrder(Vec<usize>);

impl ItemOrder {
    fn new(
        items: &[OrderingItem<'_>],
        indices: Vec<usize>,
        constraints: &[OrderingConstraint],
    ) -> Result<Self, String> {
        validate_required_item_order(items.len(), &indices, constraints).map_err(str::to_owned)?;

        Ok(Self(indices))
    }

    fn is_identity(&self) -> bool {
        self.0.iter().copied().eq(0..self.0.len())
    }
}

#[derive(Debug)]
pub struct OrderingConstraint {
    pub before_item_index: usize,
    pub after_item_index: usize,
    pub kind: OrderingConstraintKind,
}

#[derive(Clone, Copy, Debug)]
pub enum OrderingConstraintKind {
    GroupOrTestSectionOrder,
    TrailingTestModuleOrder,
    VisibilityOrder,
    TypeImplAdjacency,
    RecursiveFnAdjacency,
    CallerBeforeHelper,
    MacroBindingPreservation,
}

fn validate_required_item_order(
    item_count: usize,
    required_item_order: &[usize],
    constraints: &[OrderingConstraint],
) -> Result<(), &'static str> {
    let mut permutation = required_item_order.to_vec();
    permutation.sort_unstable();

    if !permutation.into_iter().eq(0..item_count) {
        return Err("incomplete item permutation");
    }

    let valid_constraints = constraints.iter().all(|constraint| {
        let before_position = required_item_order
            .iter()
            .position(|&item_index| item_index == constraint.before_item_index);
        let after_position = required_item_order
            .iter()
            .position(|&item_index| item_index == constraint.after_item_index);
        let (Some(before_position), Some(after_position)) = (before_position, after_position) else {
            return false;
        };

        match constraint.kind {
            OrderingConstraintKind::TypeImplAdjacency | OrderingConstraintKind::RecursiveFnAdjacency => {
                before_position.saturating_add(1) == after_position
            }
            OrderingConstraintKind::GroupOrTestSectionOrder
            | OrderingConstraintKind::TrailingTestModuleOrder
            | OrderingConstraintKind::VisibilityOrder
            | OrderingConstraintKind::CallerBeforeHelper
            | OrderingConstraintKind::MacroBindingPreservation => before_position < after_position,
        }
    });

    if valid_constraints {
        Ok(())
    } else {
        Err("invalid item identity or ordering constraint")
    }
}

struct OrderingItem<'ast> {
    metadata: ItemOrderingMetadata,
    ast: ItemAst<'ast>,
    section: TestOrderSection,
}

struct OrderingScope {
    label: String,
    body_start: LineColumn,
    test_ordering: TestOrdering,
}

enum ScopeWork<'ast> {
    Discover {
        items: &'ast [Item],
        name: String,
        test_ordering: TestOrdering,
    },
    Fix {
        items: Vec<ItemAst<'ast>>,
        scope: OrderingScope,
    },
}

#[derive(Clone, Copy)]
enum TestOrdering {
    InProdModule,
    InTestOnlyModule,
}

impl TestOrdering {
    fn for_nested_module(self, module: &ItemMod) -> Self {
        if is_test_only_module(module) {
            Self::InTestOnlyModule
        } else {
            self
        }
    }

    fn classify_item_section(self, ast: ItemAst<'_>) -> TestOrderSection {
        if let ItemAst::ModuleItem(Item::Mod(module)) = ast
            && module.content.is_some()
            && is_test_only_module(module)
        {
            return TestOrderSection::TrailingTestModules;
        }

        let test_attribute_pattern = lazy_regex::regex!(r"(?:^|_)test(?:_|$)|^rstest$");

        let is_test_fn = ast.fn_attributes().is_some_and(|attributes| {
            attributes.iter().any(|attribute| {
                attribute
                    .path()
                    .segments
                    .iter()
                    .any(|segment| test_attribute_pattern.is_match(&segment.ident.to_string()))
            })
        });

        match (self, ast.is_fn(), is_test_fn) {
            (Self::InProdModule, _, true) | (Self::InTestOnlyModule, true, false) => TestOrderSection::TrailingFns,
            (Self::InProdModule, _, false) | (Self::InTestOnlyModule, _, true | false) => {
                TestOrderSection::LeadingItems
            }
        }
    }
}

impl From<&Path> for TestOrdering {
    fn from(path: &Path) -> Self {
        if path.extension().is_none_or(|extension| extension != "rs") {
            return Self::InProdModule;
        }

        let test_file = path.file_stem().and_then(|name| name.to_str()).is_some_and(|name| {
            name == "tests" || name.starts_with("test_") || name.ends_with("_test") || name.ends_with("_tests")
        });

        if test_file {
            Self::InTestOnlyModule
        } else {
            Self::InProdModule
        }
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum TestOrderSection {
    LeadingItems,
    TrailingFns,
    TrailingTestModules,
}

fn is_test_only_module(module: &ItemMod) -> bool {
    module.attrs.iter().any(|attribute| {
        let Meta::List(meta) = &attribute.meta else {
            return false;
        };

        meta.path.is_ident("cfg")
            && syn::parse2::<syn::Path>(meta.tokens.clone()).is_ok_and(|path| path.is_ident("test"))
    })
}

#[derive(Clone, Copy)]
enum ItemAst<'ast> {
    ModuleItem(&'ast Item),
    ImplItem(&'ast ImplItem),
}

impl<'ast> ItemAst<'ast> {
    const fn is_fn(self) -> bool {
        matches!(self, Self::ModuleItem(Item::Fn(_)) | Self::ImplItem(ImplItem::Fn(_)))
    }

    const fn allows_visibility_order_with(self, other: Self) -> bool {
        match (self, other) {
            (Self::ModuleItem(Item::Use(_) | Item::Mod(ItemMod { content: None, .. })), _)
            | (_, Self::ModuleItem(Item::Use(_) | Item::Mod(ItemMod { content: None, .. }))) => false,
            (Self::ModuleItem(_), Self::ModuleItem(_))
            | (Self::ImplItem(ImplItem::Fn(_)), Self::ImplItem(ImplItem::Fn(_))) => true,
            (Self::ModuleItem(_) | Self::ImplItem(_), Self::ModuleItem(_) | Self::ImplItem(_)) => false,
        }
    }

    fn span(self) -> Span {
        match self {
            Self::ModuleItem(item) => item.span(),
            Self::ImplItem(item) => item.span(),
        }
    }

    fn fn_attributes(self) -> Option<&'ast [Attribute]> {
        match self {
            Self::ModuleItem(Item::Fn(item)) => Some(&item.attrs),
            Self::ImplItem(ImplItem::Fn(item)) => Some(&item.attrs),
            Self::ModuleItem(_) | Self::ImplItem(_) => None,
        }
    }

    const fn fn_name(self) -> Option<&'ast Ident> {
        match self {
            Self::ModuleItem(Item::Fn(item)) => Some(&item.sig.ident),
            Self::ImplItem(ImplItem::Fn(item)) => Some(&item.sig.ident),
            Self::ModuleItem(_) | Self::ImplItem(_) => None,
        }
    }
}

fn schedule_nested_scopes<'ast>(
    items: &'ast [Item],
    parent: &str,
    test_ordering: TestOrdering,
    pending: &mut Vec<ScopeWork<'ast>>,
) {
    // Stack order retains source-order siblings and fixes each parent after its children.
    for item in items.iter().rev() {
        match item {
            Item::Mod(module) => {
                let Some((brace, items)) = &module.content else {
                    continue;
                };

                let test_ordering = test_ordering.for_nested_module(module);
                let name = format!("{parent}::{}", module.ident);
                pending.push(ScopeWork::Fix {
                    items: items.iter().map(ItemAst::ModuleItem).collect(),
                    scope: OrderingScope {
                        label: format!("module {name}"),
                        body_start: brace.span.open().end(),
                        test_ordering,
                    },
                });
                pending.push(ScopeWork::Discover {
                    items,
                    name,
                    test_ordering,
                });
            }
            Item::Impl(item) if item.trait_.is_none() => {
                pending.push(ScopeWork::Fix {
                    items: item.items.iter().map(ItemAst::ImplItem).collect(),
                    scope: OrderingScope {
                        label: format!(
                            "impl {}",
                            ast::impl_target_name(item).unwrap_or_else(|| "type".to_owned())
                        ),
                        body_start: item.brace_token.span.open().end(),
                        test_ordering,
                    },
                });
            }
            Item::Const(_)
            | Item::Enum(_)
            | Item::ExternCrate(_)
            | Item::Fn(_)
            | Item::ForeignMod(_)
            | Item::Impl(_)
            | Item::Macro(_)
            | Item::Static(_)
            | Item::Struct(_)
            | Item::Trait(_)
            | Item::TraitAlias(_)
            | Item::Type(_)
            | Item::Union(_)
            | Item::Use(_)
            | Item::Verbatim(_)
            | _ => {}
        }
    }
}

fn fix_scope<'ast>(
    ctx: &FileContext<'_>,
    ast_items: impl Iterator<Item = ItemAst<'ast>>,
    scope: &OrderingScope,
    editor: &mut SourceEditor<'_>,
) -> rootcause::Result<()> {
    let ast_items: Vec<_> = ast_items.collect();

    if ast_items.len() < 2 {
        return Ok(());
    }

    let spans: Vec<_> = ast_items.iter().map(|item| item.span()).collect();
    let ranges = item_ranges::item_ranges_with_attached_comments(ctx.source, &spans, scope.body_start);
    let items: Vec<_> = ast_items
        .into_iter()
        .zip(ranges)
        .map(|(ast, range)| OrderingItem {
            metadata: ItemOrderingMetadata::new(ast, range),
            ast,
            section: scope.test_ordering.classify_item_section(ast),
        })
        .collect();

    match item_ordering::compute_required_item_order(&items) {
        Ok(order) if !order.is_identity() => reorder_scope(&items, &order.0, editor)?,
        Ok(_) => {}
        Err(message) => editor.skip(format!("{}: {message}", scope.label)),
    }

    Ok(())
}

fn reorder_scope(items: &[OrderingItem<'_>], order: &[usize], editor: &mut SourceEditor<'_>) -> rootcause::Result<()> {
    let mut replacement = String::new();

    for (slot, &index) in order.iter().enumerate() {
        let item = items
            .get(index)
            .ok_or_else(|| rootcause::report!("invalid ordering permutation"))?;
        replacement.push_str(&editor.render_range(item.metadata.range.byte_range.clone())?);

        if let (Some(current), Some(next)) = (items.get(slot), items.get(slot.saturating_add(1))) {
            let gap = current.metadata.range.byte_range.end..next.metadata.range.byte_range.start;
            replacement.push_str(&editor.render_range(gap)?);
        }
    }

    if let (Some(first), Some(last)) = (items.first(), items.last()) {
        editor.replace(
            first.metadata.range.byte_range.start..last.metadata.range.byte_range.end,
            replacement,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[rstest::rstest]
    #[case(vec![0, 1], Vec::new(), Ok(()))]
    #[case(vec![0], Vec::new(), Err("incomplete item permutation"))]
    #[case(vec![0, 0], Vec::new(), Err("incomplete item permutation"))]
    #[case(
        vec![0, 1],
        vec![OrderingConstraint {
            before_item_index: 0,
            after_item_index: 1,
            kind: OrderingConstraintKind::CallerBeforeHelper,
        }],
        Ok(()),
    )]
    #[case(
        vec![1, 0],
        vec![OrderingConstraint {
            before_item_index: 0,
            after_item_index: 1,
            kind: OrderingConstraintKind::CallerBeforeHelper,
        }],
        Err("invalid item identity or ordering constraint"),
    )]
    fn test_ordering_rule_when_required_item_order_is_validated_reports_invalid_item_order(
        #[case] required_item_order: Vec<usize>,
        #[case] constraints: Vec<OrderingConstraint>,
        #[case] expected: Result<(), &'static str>,
    ) {
        assert_eq!(
            validate_required_item_order(2, &required_item_order, &constraints),
            expected
        );
    }
}
