//! Arrange declarations so readers can find related code together and read callers before their helpers.
//!
//! Put crate declarations, imports, and external module declarations first, followed by global assembly,
//! constants and statics, type aliases, and then the remaining definitions.
//! Keep each type beside its own methods, followed by its trait implementations.
//! Within each group and among a type's methods, put more widely visible code before private code.
//!
//! Place private helper functions after the functions that use them. When several functions share a helper,
//! prefer placing it after a public caller. Keep helpers that call each other together.
//! Keep the existing order when these preferences do not require a change.
//!
//! Put inline modules after all other declarations, in alphabetical order, with `tests` last.
//! In production code, put test functions after other definitions, before inline modules.
//! In test-only files and modules, put tests before their helper functions.
//! Leave import ordering and the order of module declarations in separate files to the formatter.
//!
//! Move attached comments and attributes with their declarations. Preserve order where macros may depend on it.
//! If the requirements cannot be met together, report the conflict and leave that part unchanged.

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
use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::ast;
use crate::cmds::rsl::ast::ItemGroup;
use crate::cmds::rsl::ast::ItemKind;
use crate::cmds::rsl::ast::VisibilityClass;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;
use crate::cmds::rsl::source::SourceRange;
use crate::cmds::rsl::source::item_ranges_with_attached_comments;

mod fn_references;
mod item_ordering;

pub(super) struct OrderingRule;

impl Rule for OrderingRule {
    fn id(&self) -> &'static str {
        "ordering-rule"
    }

    fn analyze<'source>(&self, ctx: &FileContext<'source>) -> rootcause::Result<RulePlan<'source>> {
        let mut plan = RulePlan {
            edits: SourceEditor::new(ctx.source),
            findings: Vec::new(),
        };
        let test_ordering = TestOrdering::from(ctx.path);
        let mut pending = vec![
            ScopeWork::Analyze {
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
                ScopeWork::Analyze { items, scope } => {
                    analyze_scope(ctx, items.into_iter(), &scope, &mut plan)?;
                }
            }
        }

        Ok(plan)
    }
}

struct Change<'scope> {
    span: Span,
    scope: &'scope str,
}

impl Change<'_> {
    fn add_to_plan(
        &self,
        items: &[OrderingItem<'_>],
        order: &ItemOrder,
        plan: &mut RulePlan<'_>,
    ) -> rootcause::Result<()> {
        reorder_scope(items, &order.0, &mut plan.edits)?;
        plan.record_fixable(self);
        Ok(())
    }
}

impl RuleChange for Change<'_> {
    fn span(&self) -> Span {
        self.span
    }

    fn description(&self) -> String {
        format!("reorder declarations in {}", self.scope)
    }
}

struct OrderingConflict(String);

impl From<OrderingConflict> for FixBlockReason {
    fn from(conflict: OrderingConflict) -> Self {
        Self {
            code: "ordering-conflict",
            message: format!(
                "The declarations have incompatible ordering requirements: {}.",
                conflict.0
            ),
            help: "Review the conflicting declarations and their required order manually.",
        }
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
    GroupOrSectionOrder,
    InlineModuleOrder,
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
            OrderingConstraintKind::GroupOrSectionOrder
            | OrderingConstraintKind::InlineModuleOrder
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
    section: OrderingSection,
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
    Analyze {
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

    fn classify_item_section(self, ast: ItemAst<'_>) -> OrderingSection {
        if let ItemAst::ModuleItem(Item::Mod(module)) = ast
            && module.content.is_some()
        {
            return if module.ident == "tests" {
                OrderingSection::TestsModule
            } else {
                OrderingSection::InlineModules
            };
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
            (Self::InProdModule, _, true) | (Self::InTestOnlyModule, true, false) => OrderingSection::TrailingFns,
            (Self::InProdModule, _, false) | (Self::InTestOnlyModule, _, true | false) => OrderingSection::LeadingItems,
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
enum OrderingSection {
    LeadingItems,
    TrailingFns,
    InlineModules,
    TestsModule,
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
    fn ordering_metadata(self, range: SourceRange) -> ItemOrderingMetadata {
        let (kind, label, visibility) = match self {
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

        ItemOrderingMetadata {
            range,
            label,
            kind,
            visibility,
        }
    }

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
                pending.push(ScopeWork::Analyze {
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
                pending.push(ScopeWork::Analyze {
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

fn analyze_scope<'ast>(
    ctx: &FileContext<'_>,
    ast_items: impl Iterator<Item = ItemAst<'ast>>,
    scope: &OrderingScope,
    plan: &mut RulePlan<'_>,
) -> rootcause::Result<()> {
    let ast_items: Vec<_> = ast_items.collect();

    if ast_items.len() < 2 {
        return Ok(());
    }

    let spans: Vec<_> = ast_items.iter().map(|item| item.span()).collect();
    let Some(&location) = spans.first() else {
        return Ok(());
    };
    let ranges = item_ranges_with_attached_comments(ctx.source, &spans, scope.body_start);
    let items: Vec<_> = ast_items
        .into_iter()
        .zip(ranges)
        .map(|(ast, range)| OrderingItem {
            metadata: ast.ordering_metadata(range),
            ast,
            section: scope.test_ordering.classify_item_section(ast),
        })
        .collect();

    let change = Change {
        span: location,
        scope: &scope.label,
    };
    match item_ordering::compute_required_item_order(&items) {
        Ok(order) if !order.is_identity() => {
            change.add_to_plan(&items, &order, plan)?;
        }
        Ok(_) => {}
        Err(message) => plan.record_blocked(&change, OrderingConflict(message)),
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
