//! Deterministic item ordering, with one complete arrangement per affected scope.
//!
//! ```text
//! extern_crate -> use -> foreign_mod/mod -> #[cfg(test)] mod tests
//! -> global_asm -> const/static -> ty_alias -> macros/types/traits/impls/functions
//! ```
//!
//! Keep contiguous blocks: `type -> inherent impls -> trait impls`, and mutually recursive private helpers.
//! Within each group or inherent impl: `pub -> pub(crate) -> restricted -> private`; imports follow the formatter.
//! Test-only files (`tests.rs`, `test_*.rs`, `*_test.rs`, `*_tests.rs`) and inline `#[cfg(test)]`
//! modules inherit `tests -> helpers`; production scopes keep test functions last.
//! Test attribute path segments match `(?:^|_)test(?:_|$)|^rstest$`; arguments do not classify functions.
//! Sections override visibility and callers. Calls and recursive blocks stay within their section.
//! Private helpers follow their earliest eligible caller; shared helpers prefer public callers.
//! Duplicate-name impls attach to the last preceding declaration, without cfg evaluation.
//!
//! Exception: keep macros in their normal group and defer consuming inline modules after them.
//! Preserve lexical shadowing: inner definitions shadow outer names from their declaration onward.
//!
//! Diagnostics list all immediate items in required order; conflicts emit no move instructions.
//! Rich item metadata, original/required sequences, and ordering reasons remain in the Rust payload.
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

use proc_macro2::Ident;
use proc_macro2::LineColumn;
use proc_macro2::Span;
use ranges::SourceRange;
use regex::Regex;
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

mod calls;
mod order;
mod ranges;

#[cfg(test)]
mod tests;

pub struct OrderingRule;

impl TypedRule for OrderingRule {
    type Violation = OrderingRuleViolation;

    fn code() -> &'static str {
        "ordering_rule"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let attribute_pattern = match Regex::new(r"(?:^|_)test(?:_|$)|^rstest$") {
            Ok(pattern) => pattern,
            Err(error) => {
                return vec![OrderingRuleViolation {
                    location: Location::from_span(ctx.path, Span::call_site()),
                    scope: "module crate".to_owned(),
                    depth: 0,
                    items: Vec::new(),
                    details: Arrangement::Conflict {
                        message: format!("invalid test attribute pattern: {error}"),
                        reasons: Vec::new(),
                    },
                }];
            }
        };
        let tests = TestContext {
            kind: ScopeKind::from_path(ctx.path),
            attribute_pattern: &attribute_pattern,
        };
        let mut violations = Vec::new();
        check_scope(
            ctx,
            ctx.file.items.iter().map(SyntaxItem::Module),
            ScopeDescription {
                name: "module crate".to_owned(),
                span: Span::call_site(),
                body_start: LineColumn { line: 1, column: 0 },
                tests,
            },
            0,
            &mut violations,
        );
        check_nested_scopes(ctx, &ctx.file.items, "crate", 0, tests, &mut violations);
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
    pub items: Vec<ItemDetails>,
    pub details: Arrangement,
}

impl TypedRuleViolation for OrderingRuleViolation {
    type Rule = OrderingRule;
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct ItemDetails {
    pub range: SourceRange,
    pub label: String,
    pub kind: Option<ItemKind>,
    pub group: ItemGroup,
    pub visibility: Option<VisibilityClass>,
    /// Index of the first member in this item's atomic block, in the original snapshot.
    pub block: usize,
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum Arrangement {
    Ordered {
        original: Vec<usize>,
        required: Vec<usize>,
        reasons: Vec<OrderingRuleReason>,
    },
    Conflict {
        message: String,
        reasons: Vec<OrderingRuleReason>,
    },
}

impl Arrangement {
    fn validate(self, items: &[ScopeItem<'_>]) -> Self {
        let reasons = match &self {
            Self::Ordered { reasons, .. } | Self::Conflict { reasons, .. } => reasons,
        };
        let invalid_identity = reasons
            .iter()
            .any(|reason| reason.before >= items.len() || reason.after >= items.len());
        let invalid_metadata = items.iter().any(|item| {
            item.details.label.is_empty()
                || item.details.kind.is_some_and(|kind| kind.group() != item.details.group)
                || item.details.range.bytes.is_empty()
        });
        let error = if invalid_identity || invalid_metadata {
            Some("invalid item identity or ordering constraint")
        } else {
            match &self {
                Self::Ordered { original, required, .. } => validate_sequence(original, required, reasons).err(),
                Self::Conflict { .. } => None,
            }
        };
        if let Some(message) = error {
            let reasons = match self {
                Self::Ordered { reasons, .. } | Self::Conflict { reasons, .. } => reasons,
            };
            return Self::Conflict {
                message: message.to_owned(),
                reasons,
            };
        }
        self
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct OrderingRuleReason {
    pub before: usize,
    pub after: usize,
    pub preference: Preference,
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum Preference {
    Group,
    Visibility,
    TypeBlock,
    RecursiveBlock,
    Caller,
    MacroScope,
}

fn validate_sequence(
    original: &[usize],
    required: &[usize],
    reasons: &[OrderingRuleReason],
) -> Result<(), &'static str> {
    let mut permutation = required.to_vec();
    permutation.sort_unstable();
    if permutation != original {
        return Err("incomplete item permutation");
    }
    let valid_constraints = reasons.iter().all(|reason| {
        let before = required.iter().position(|&identity| identity == reason.before);
        let after = required.iter().position(|&identity| identity == reason.after);
        let (Some(before), Some(after)) = (before, after) else {
            return false;
        };
        match reason.preference {
            Preference::TypeBlock | Preference::RecursiveBlock => before.saturating_add(1) == after,
            Preference::Group | Preference::Visibility | Preference::Caller | Preference::MacroScope => before < after,
        }
    });
    if valid_constraints {
        Ok(())
    } else {
        Err("invalid item identity or ordering constraint")
    }
}

struct ScopeItem<'ast> {
    details: ItemDetails,
    syntax: SyntaxItem<'ast>,
    section: Section,
}

struct ScopeDescription<'a> {
    name: String,
    span: Span,
    body_start: LineColumn,
    tests: TestContext<'a>,
}

#[derive(Clone, Copy)]
enum ScopeKind {
    Production,
    TestOnly,
}

impl ScopeKind {
    fn from_path(path: &Path) -> Self {
        if path.extension().is_none_or(|extension| extension != "rs") {
            return Self::Production;
        }
        let test_file = path.file_stem().and_then(|name| name.to_str()).is_some_and(|name| {
            name == "tests" || name.starts_with("test_") || name.ends_with("_test") || name.ends_with("_tests")
        });
        if test_file { Self::TestOnly } else { Self::Production }
    }
}

#[derive(Clone, Copy)]
struct TestContext<'a> {
    kind: ScopeKind,
    attribute_pattern: &'a Regex,
}

impl TestContext<'_> {
    fn nested(self, module: &ItemMod) -> Self {
        let test_module = module.attrs.iter().any(|attribute| {
            let Meta::List(meta) = &attribute.meta else {
                return false;
            };
            meta.path.is_ident("cfg")
                && syn::parse2::<syn::Path>(meta.tokens.clone()).is_ok_and(|path| path.is_ident("test"))
        });
        Self {
            kind: if test_module { ScopeKind::TestOnly } else { self.kind },
            ..self
        }
    }

    fn section(self, syntax: SyntaxItem<'_>) -> Section {
        let test = syntax.function_attributes().is_some_and(|attributes| {
            attributes.iter().any(|attribute| {
                attribute
                    .path()
                    .segments
                    .iter()
                    .any(|segment| self.attribute_pattern.is_match(&segment.ident.to_string()))
            })
        });
        match (self.kind, syntax.is_function(), test) {
            (ScopeKind::Production, _, true) | (ScopeKind::TestOnly, true, false) => Section::Trailing,
            (ScopeKind::Production, _, false) | (ScopeKind::TestOnly, _, true | false) => Section::Leading,
        }
    }
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Section {
    Leading,
    Trailing,
}

#[derive(Clone, Copy)]
enum SyntaxItem<'ast> {
    Module(&'ast Item),
    Associated(&'ast ImplItem),
}

impl<'ast> SyntaxItem<'ast> {
    const fn is_function(self) -> bool {
        matches!(self, Self::Module(Item::Fn(_)) | Self::Associated(ImplItem::Fn(_)))
    }

    fn span(self) -> Span {
        match self {
            Self::Module(item) => item.span(),
            Self::Associated(item) => item.span(),
        }
    }

    fn function_attributes(self) -> Option<&'ast [Attribute]> {
        match self {
            Self::Module(Item::Fn(item)) => Some(&item.attrs),
            Self::Associated(ImplItem::Fn(item)) => Some(&item.attrs),
            Self::Module(_) | Self::Associated(_) => None,
        }
    }

    const fn function_name(self) -> Option<&'ast Ident> {
        match self {
            Self::Module(Item::Fn(item)) => Some(&item.sig.ident),
            Self::Associated(ImplItem::Fn(item)) => Some(&item.sig.ident),
            Self::Module(_) | Self::Associated(_) => None,
        }
    }
}

fn check_nested_scopes(
    ctx: &FileContext<'_>,
    items: &[Item],
    name: &str,
    depth: usize,
    tests: TestContext<'_>,
    violations: &mut Vec<OrderingRuleViolation>,
) {
    for item in items {
        match item {
            Item::Mod(module) => {
                if let Some((brace, nested)) = &module.content {
                    let tests = tests.nested(module);
                    let name = format!("{name}::{}", module.ident);
                    let depth = depth.saturating_add(1);
                    check_scope(
                        ctx,
                        nested.iter().map(SyntaxItem::Module),
                        ScopeDescription {
                            name: format!("module {name}"),
                            span: module.mod_token.span,
                            body_start: brace.span.open().end(),
                            tests,
                        },
                        depth,
                        violations,
                    );
                    check_nested_scopes(ctx, nested, &name, depth, tests, violations);
                }
            }
            Item::Impl(item) if item.trait_.is_none() => check_scope(
                ctx,
                item.items.iter().map(SyntaxItem::Associated),
                ScopeDescription {
                    name: format!(
                        "impl {}",
                        ast::impl_target_name(item).unwrap_or_else(|| "type".to_owned())
                    ),
                    span: item.impl_token.span,
                    body_start: item.brace_token.span.open().end(),
                    tests,
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
    syntax: impl Iterator<Item = SyntaxItem<'ast>>,
    scope: ScopeDescription<'_>,
    depth: usize,
    violations: &mut Vec<OrderingRuleViolation>,
) {
    let syntax: Vec<_> = syntax.collect();
    if syntax.len() < 2 {
        return;
    }

    let spans: Vec<_> = syntax.iter().map(|item| item.span()).collect();
    let source_ranges = ranges::item_ranges(ctx.source, &spans, scope.body_start);
    let mut items: Vec<_> = syntax
        .into_iter()
        .zip(source_ranges)
        .enumerate()
        .map(|(index, (syntax, range))| scope_item(syntax, index, range, scope.tests.section(syntax)))
        .collect();

    let arrangement = order::arrange(&mut items).validate(&items);
    if matches!(&arrangement, Arrangement::Ordered { original, required, .. } if original == required) {
        return;
    }

    violations.push(OrderingRuleViolation {
        location: Location::from_span(ctx.path, scope.span),
        scope: scope.name,
        depth,
        items: items.into_iter().map(|item| item.details).collect(),
        details: arrangement,
    });
}

fn scope_item(syntax: SyntaxItem<'_>, index: usize, range: SourceRange, section: Section) -> ScopeItem<'_> {
    let (kind, label, visibility) = match syntax {
        SyntaxItem::Module(item) => {
            let kind = ast::classify_item(item);
            let label = kind.map_or_else(|| "opaque item".to_owned(), |kind| ast::item_label(item, kind));
            (kind, label, ast::item_visibility(item))
        }
        SyntaxItem::Associated(item) => associated_details(item),
    };
    ScopeItem {
        syntax,
        section,
        details: ItemDetails {
            range,
            label,
            kind,
            group: kind.map_or(ItemGroup::Items, ItemKind::group),
            visibility,
            block: index,
        },
    }
}

fn associated_details(item: &ImplItem) -> (Option<ItemKind>, String, Option<VisibilityClass>) {
    match item {
        ImplItem::Fn(item) => (
            Some(ItemKind::Fn),
            format!("fn {}", item.sig.ident),
            Some((&item.vis).into()),
        ),
        ImplItem::Const(item) => (
            Some(ItemKind::Const),
            format!("const {}", item.ident),
            Some((&item.vis).into()),
        ),
        ImplItem::Type(item) => (
            Some(ItemKind::TypeAlias),
            format!("type {}", item.ident),
            Some((&item.vis).into()),
        ),
        ImplItem::Macro(_) | ImplItem::Verbatim(_) | _ => (None, "opaque item".to_owned(), None),
    }
}
