//! Deterministic item ordering, with one complete arrangement per affected scope.
//!
//! ```text
//! extern_crate -> use -> foreign_mod/mod -> #[cfg(test)] mod tests;
//! -> global_asm -> const/static -> ty_alias -> macros/types/traits/impls/functions
//! -> inline #[cfg(test)] mod tests { ... }
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
//! Ordering preferences yield to macro preservation. Incompatible adjacency emits a conflict without moves.
//!
//! Diagnostics list all immediate items in required order; conflicts emit no move instructions.
//! Rich item metadata, original/required sequences, and ordering constraints remain in the Rust payload.
//!
//! ```text
//! src/lib.rs:1:1,ordering_rule,module crate: order=[14-15,1-2,4-12]
//! ```
//!
//! Apply inner scopes first, retaining original-snapshot identities as earlier edits shift positions.
//! Ranges include attributes and attached comments: `9-11` is inclusive; `9:3-11:2` uses one-based columns
//! and an exclusive end. Same-line trailing comments attach backward; contiguous leading comments attach forward.
//! Blank-separated comments and inner docs stay scope-owned; whitespace gaps stay in their slots.

use std::path::Path;

use item_source_ranges::SourceRange;
use proc_macro2::Ident;
use proc_macro2::LineColumn;
use proc_macro2::Span;
use syn::Attribute;
use syn::ImplItem;
use syn::Item;
use syn::ItemMod;
use syn::Meta;
use syn::spanned::Spanned;

use crate::cmds::rsl::ast;
use crate::cmds::rsl::ast::ItemGroup;
use crate::cmds::rsl::ast::ItemKind;
use crate::cmds::rsl::ast::VisibilityClass;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;
use crate::cmds::rsl::rules::common::Location;

mod fn_references;
mod item_ordering;
mod item_source_ranges;

#[cfg(test)]
mod tests;

pub struct OrderingRule;

impl TypedRule for OrderingRule {
    type Violation = OrderingRuleViolation;

    fn code() -> &'static str {
        "ordering_rule"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let test_ordering = TestOrdering::from(ctx.path);
        let mut violations = Vec::new();
        check_scope(
            ctx,
            ctx.file.items.iter().map(ItemAst::ModuleItem),
            OrderingScope {
                label: "module crate".to_owned(),
                diagnostic_span: Span::call_site(),
                body_start: LineColumn { line: 1, column: 0 },
                test_ordering,
            },
            0,
            &mut violations,
        );
        check_nested_scopes(ctx, &ctx.file.items, "crate", 0, test_ordering, &mut violations);
        violations.sort_by_key(|violation| {
            (
                std::cmp::Reverse(violation.depth),
                violation.location.line,
                violation.location.column,
            )
        });
        violations
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct OrderingRuleViolation {
    pub location: Location,
    pub scope: String,
    pub depth: usize,
    pub items: Vec<ItemOrderingMetadata>,
    pub outcome: OrderingOutcome,
}

impl TypedRuleViolation for OrderingRuleViolation {
    type Rule = OrderingRule;
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct ItemOrderingMetadata {
    pub range: SourceRange,
    pub label: String,
    pub kind: Option<ItemKind>,
    pub group: ItemGroup,
    pub visibility: Option<VisibilityClass>,
    /// Index of the first member in this item's atomic block, in the original snapshot.
    pub block_first_item_index: usize,
}

impl ItemOrderingMetadata {
    fn new(ast: ItemAst<'_>, source_item_index: usize, range: SourceRange) -> Self {
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
            group: kind.map_or(ItemGroup::Items, ItemKind::group),
            visibility,
            block_first_item_index: source_item_index,
        }
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum OrderingOutcome {
    Computed {
        source_item_order: Vec<usize>,
        required_item_order: Vec<usize>,
        constraints: Vec<OrderingConstraint>,
    },
    Failed {
        message: String,
        constraints: Vec<OrderingConstraint>,
    },
}

impl OrderingOutcome {
    fn validate(self, items: &[OrderingItem<'_>]) -> Self {
        let constraints = match &self {
            Self::Computed { constraints, .. } | Self::Failed { constraints, .. } => constraints,
        };
        let invalid_identity = constraints.iter().any(|constraint| {
            constraint.before_item_index >= items.len() || constraint.after_item_index >= items.len()
        });
        let invalid_metadata = items.iter().any(|item| {
            item.metadata.label.is_empty()
                || item
                    .metadata
                    .kind
                    .is_some_and(|kind| kind.group() != item.metadata.group)
                || item.metadata.range.byte_range.is_empty()
        });
        let error = if invalid_identity || invalid_metadata {
            Some("invalid item identity or ordering constraint")
        } else {
            match &self {
                Self::Computed {
                    source_item_order,
                    required_item_order,
                    ..
                } => validate_required_item_order(source_item_order, required_item_order, constraints).err(),
                Self::Failed { .. } => None,
            }
        };
        if let Some(message) = error {
            let constraints = match self {
                Self::Computed { constraints, .. } | Self::Failed { constraints, .. } => constraints,
            };
            return Self::Failed {
                message: message.to_owned(),
                constraints,
            };
        }
        self
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct OrderingConstraint {
    pub before_item_index: usize,
    pub after_item_index: usize,
    pub kind: OrderingConstraintKind,
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum OrderingConstraintKind {
    GroupOrTestSectionOrder,
    VisibilityOrder,
    TypeImplAdjacency,
    RecursiveFnAdjacency,
    CallerBeforeHelper,
    MacroBindingPreservation,
}

fn validate_required_item_order(
    source_item_order: &[usize],
    required_item_order: &[usize],
    constraints: &[OrderingConstraint],
) -> Result<(), &'static str> {
    let mut permutation = required_item_order.to_vec();
    permutation.sort_unstable();
    if permutation != source_item_order {
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
    diagnostic_span: Span,
    body_start: LineColumn,
    test_ordering: TestOrdering,
}

#[derive(Clone, Copy)]
enum TestOrdering {
    InProdModule,
    InTestOnlyModule,
}

impl TestOrdering {
    fn for_nested_module(self, module: &ItemMod) -> Self {
        let is_test_only_module = module.attrs.iter().any(|attribute| {
            let Meta::List(meta) = &attribute.meta else {
                return false;
            };
            meta.path.is_ident("cfg")
                && syn::parse2::<syn::Path>(meta.tokens.clone()).is_ok_and(|path| path.is_ident("test"))
        });
        if is_test_only_module {
            Self::InTestOnlyModule
        } else {
            self
        }
    }

    fn classify_item_section(self, ast: ItemAst<'_>) -> TestOrderSection {
        if let ItemAst::ModuleItem(Item::Mod(module)) = ast
            && module.content.is_some()
            && ast::is_test_module_declaration(module)
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

fn check_nested_scopes(
    ctx: &FileContext<'_>,
    items: &[Item],
    name: &str,
    depth: usize,
    test_ordering: TestOrdering,
    violations: &mut Vec<OrderingRuleViolation>,
) {
    for item in items {
        match item {
            Item::Mod(module) => {
                if let Some((brace, nested)) = &module.content {
                    let test_ordering = test_ordering.for_nested_module(module);
                    let name = format!("{name}::{}", module.ident);
                    let depth = depth.saturating_add(1);
                    check_scope(
                        ctx,
                        nested.iter().map(ItemAst::ModuleItem),
                        OrderingScope {
                            label: format!("module {name}"),
                            diagnostic_span: module.mod_token.span,
                            body_start: brace.span.open().end(),
                            test_ordering,
                        },
                        depth,
                        violations,
                    );
                    check_nested_scopes(ctx, nested, &name, depth, test_ordering, violations);
                }
            }
            Item::Impl(item) if item.trait_.is_none() => check_scope(
                ctx,
                item.items.iter().map(ItemAst::ImplItem),
                OrderingScope {
                    label: format!(
                        "impl {}",
                        ast::impl_target_name(item).unwrap_or_else(|| "type".to_owned())
                    ),
                    diagnostic_span: item.impl_token.span,
                    body_start: item.brace_token.span.open().end(),
                    test_ordering,
                },
                depth.saturating_add(1),
                violations,
            ),
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

fn check_scope<'ast>(
    ctx: &FileContext<'_>,
    ast_items: impl Iterator<Item = ItemAst<'ast>>,
    scope: OrderingScope,
    depth: usize,
    violations: &mut Vec<OrderingRuleViolation>,
) {
    let ast_items: Vec<_> = ast_items.collect();
    if ast_items.len() < 2 {
        return;
    }

    let spans: Vec<_> = ast_items.iter().map(|item| item.span()).collect();
    let item_source_ranges =
        item_source_ranges::item_ranges_with_attached_comments(ctx.source, &spans, scope.body_start);

    let mut items: Vec<_> = ast_items
        .into_iter()
        .zip(item_source_ranges)
        .enumerate()
        .map(|(source_item_index, (ast, range))| OrderingItem {
            metadata: ItemOrderingMetadata::new(ast, source_item_index, range),
            ast,
            section: scope.test_ordering.classify_item_section(ast),
        })
        .collect();

    let ordering_outcome = item_ordering::compute_required_item_order(&mut items).validate(&items);

    if matches!(&ordering_outcome, OrderingOutcome::Computed { source_item_order, required_item_order, .. } if source_item_order == required_item_order)
    {
        return;
    }

    violations.push(OrderingRuleViolation {
        location: Location::from_span(ctx.path, scope.diagnostic_span),
        scope: scope.label,
        depth,
        items: items.into_iter().map(|item| item.metadata).collect(),
        outcome: ordering_outcome,
    });
}
