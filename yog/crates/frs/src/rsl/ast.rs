//! Shared syntax classification helpers for rsl rules.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use proc_macro2::Span;
use proc_macro2::TokenTree;
use serde::Serialize;
use syn::Item;
use syn::spanned::Spanned;

#[derive(Clone, Copy, Debug, strum::Display, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ItemGroup {
    ExternCrate,
    Use,
    Modules,
    GlobalAsm,
    Constants,
    Aliases,
    Items,
}

#[derive(Clone, Copy, Debug, Eq, strum::IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum ItemKind {
    ExternCrate,
    Use,
    ForeignMod,
    Mod,
    GlobalAsm,
    Const,
    Static,
    #[strum(to_string = "ty_alias")]
    TypeAlias,
    Macro,
    Enum,
    Struct,
    Union,
    Trait,
    TraitAlias,
    Impl,
    Fn,
}

impl Serialize for ItemKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str((*self).into())
    }
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        self.into()
    }

    pub const fn group(self) -> ItemGroup {
        match self {
            Self::ExternCrate => ItemGroup::ExternCrate,
            Self::Use => ItemGroup::Use,
            Self::ForeignMod | Self::Mod => ItemGroup::Modules,
            Self::GlobalAsm => ItemGroup::GlobalAsm,
            Self::Const | Self::Static => ItemGroup::Constants,
            Self::TypeAlias => ItemGroup::Aliases,
            Self::Macro
            | Self::Enum
            | Self::Struct
            | Self::Union
            | Self::Trait
            | Self::TraitAlias
            | Self::Impl
            | Self::Fn => ItemGroup::Items,
        }
    }
}

#[derive(Clone, Copy, Debug, strum::Display, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum VisibilityClass {
    #[serde(rename = "pub")]
    #[strum(to_string = "pub")]
    Public,
    #[serde(rename = "pub(crate)")]
    #[strum(to_string = "pub(crate)")]
    Crate,
    #[serde(rename = "restricted")]
    #[strum(to_string = "restricted")]
    Restricted,
    #[serde(rename = "private")]
    #[strum(to_string = "private")]
    Private,
}

impl From<&syn::Visibility> for VisibilityClass {
    fn from(visibility: &syn::Visibility) -> Self {
        match visibility {
            syn::Visibility::Public(_) => Self::Public,
            syn::Visibility::Restricted(restricted) => {
                if restricted.path.is_ident("crate") {
                    Self::Crate
                } else if restricted.path.is_ident("self") {
                    Self::Private
                } else {
                    Self::Restricted
                }
            }
            syn::Visibility::Inherited => Self::Private,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClassifiedItem {
    pub kind: ItemKind,
    pub span: Span,
}

pub fn type_definition(item: &Item) -> Option<(String, ItemKind, VisibilityClass)> {
    match item {
        Item::Enum(item) => Some((item.ident.to_string(), ItemKind::Enum, VisibilityClass::from(&item.vis))),
        Item::Struct(item) => Some((
            item.ident.to_string(),
            ItemKind::Struct,
            VisibilityClass::from(&item.vis),
        )),
        Item::Union(item) => Some((
            item.ident.to_string(),
            ItemKind::Union,
            VisibilityClass::from(&item.vis),
        )),
        Item::Const(_)
        | Item::ExternCrate(_)
        | Item::Fn(_)
        | Item::ForeignMod(_)
        | Item::Impl(_)
        | Item::Macro(_)
        | Item::Mod(_)
        | Item::Static(_)
        | Item::Trait(_)
        | Item::TraitAlias(_)
        | Item::Type(_)
        | Item::Use(_)
        | Item::Verbatim(_)
        | _ => None,
    }
}

pub fn impl_target_name(item_impl: &syn::ItemImpl) -> Option<String> {
    let syn::Type::Path(type_path) = item_impl.self_ty.as_ref() else {
        return None;
    };
    if type_path.qself.is_some() || type_path.path.leading_colon.is_some() || type_path.path.segments.len() != 1 {
        return None;
    }
    type_path.path.segments.first().map(|segment| segment.ident.to_string())
}

pub fn item_visibility(item: &Item) -> Option<VisibilityClass> {
    let visibility = match item {
        Item::Const(item) => &item.vis,
        Item::Enum(item) => &item.vis,
        Item::ExternCrate(item) => &item.vis,
        Item::Fn(item) => &item.vis,
        Item::Mod(item) => &item.vis,
        Item::Static(item) => &item.vis,
        Item::Struct(item) => &item.vis,
        Item::Trait(item) => &item.vis,
        Item::TraitAlias(item) => &item.vis,
        Item::Type(item) => &item.vis,
        Item::Union(item) => &item.vis,
        Item::Use(item) => &item.vis,
        Item::Macro(_) | Item::Impl(_) | Item::Verbatim(_) | _ => {
            return None;
        }
    };
    Some(VisibilityClass::from(visibility))
}

pub fn item_label(item: &Item, kind: ItemKind) -> String {
    let name = match item {
        Item::Const(item) => Some(item.ident.to_string()),
        Item::Enum(item) => Some(item.ident.to_string()),
        Item::ExternCrate(item) => Some(item.ident.to_string()),
        Item::Fn(item) => Some(item.sig.ident.to_string()),
        Item::Mod(item) => Some(item.ident.to_string()),
        Item::Static(item) => Some(item.ident.to_string()),
        Item::Struct(item) => Some(item.ident.to_string()),
        Item::Trait(item) => Some(item.ident.to_string()),
        Item::TraitAlias(item) => Some(item.ident.to_string()),
        Item::Type(item) => Some(item.ident.to_string()),
        Item::Union(item) => Some(item.ident.to_string()),
        Item::Use(_) | Item::ForeignMod(_) | Item::Macro(_) | Item::Impl(_) | Item::Verbatim(_) | _ => None,
    };
    name.map_or_else(|| kind.label().to_owned(), |name| format!("{} {name}", kind.label()))
}

pub fn impl_order_label(item: &Item) -> String {
    if let Item::Impl(item_impl) = item {
        let target = self::impl_target_name(item_impl).unwrap_or_else(|| "type".to_owned());
        if item_impl.trait_.is_some() {
            format!("trait impl {target}")
        } else {
            format!("inherent impl {target}")
        }
    } else {
        self::classify_item(item).map_or_else(
            || "item".to_owned(),
            |classified| self::item_label(item, classified.kind),
        )
    }
}

pub fn item_span(item: &Item) -> Span {
    match item {
        Item::Const(item) => item.const_token.span(),
        Item::Enum(item) => item.enum_token.span(),
        Item::ExternCrate(item) => item.extern_token.span(),
        Item::Fn(item) => item.sig.fn_token.span(),
        Item::ForeignMod(item) => item.abi.extern_token.span(),
        Item::Impl(item) => item.impl_token.span(),
        Item::Macro(item) => item.mac.path.span(),
        Item::Mod(item) => item.mod_token.span(),
        Item::Static(item) => item.static_token.span(),
        Item::Struct(item) => item.struct_token.span(),
        Item::Trait(item) => item.trait_token.span(),
        Item::TraitAlias(item) => item.trait_token.span(),
        Item::Type(item) => item.type_token.span(),
        Item::Union(item) => item.union_token.span(),
        Item::Use(item) => item.use_token.span(),
        Item::Verbatim(tokens) => self::explicit_macro_span(tokens).unwrap_or_else(|| tokens.span()),
        _ => item.span(),
    }
}

pub fn classify_item(item: &Item) -> Option<ClassifiedItem> {
    let kind = match item {
        Item::ExternCrate(_) => ItemKind::ExternCrate,
        Item::Use(_) => ItemKind::Use,
        Item::ForeignMod(_) => ItemKind::ForeignMod,
        Item::Mod(_) => ItemKind::Mod,
        Item::Macro(item_macro) if self::is_global_asm(item_macro) => ItemKind::GlobalAsm,
        Item::Const(_) => ItemKind::Const,
        Item::Static(_) => ItemKind::Static,
        Item::Type(_) => ItemKind::TypeAlias,
        Item::Macro(item_macro) if item_macro.ident.is_some() => ItemKind::Macro,
        Item::Enum(_) => ItemKind::Enum,
        Item::Struct(_) => ItemKind::Struct,
        Item::Union(_) => ItemKind::Union,
        Item::Trait(_) => ItemKind::Trait,
        Item::TraitAlias(_) => ItemKind::TraitAlias,
        Item::Impl(_) => ItemKind::Impl,
        Item::Fn(_) => ItemKind::Fn,
        Item::Verbatim(tokens) if self::explicit_macro_span(tokens).is_some() => ItemKind::Macro,
        Item::Macro(_) | Item::Verbatim(_) | _ => return None,
    };

    Some(ClassifiedItem {
        kind,
        span: self::item_span(item),
    })
}

pub(super) fn is_test_module(item: &Item) -> bool {
    let Item::Mod(module) = item else {
        return false;
    };

    module.ident == "tests"
        && module.attrs.iter().any(|attribute| {
            let syn::Meta::List(meta) = &attribute.meta else {
                return false;
            };
            meta.path.is_ident("cfg")
                && syn::parse2::<syn::Path>(meta.tokens.clone()).is_ok_and(|path| path.is_ident("test"))
        })
}

fn is_global_asm(item: &syn::ItemMacro) -> bool {
    item.mac
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "global_asm")
}

fn explicit_macro_span(tokens: &proc_macro2::TokenStream) -> Option<Span> {
    let mut tokens = tokens.clone().into_iter().peekable();
    loop {
        match tokens.next()? {
            TokenTree::Punct(punct) if punct.as_char() == '#' => {
                if !matches!(tokens.next(), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Bracket)
                {
                    return None;
                }
            }
            TokenTree::Ident(ident) if ident == "pub" => {
                if matches!(tokens.peek(), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Parenthesis)
                {
                    tokens.next();
                }
            }
            TokenTree::Ident(ident) if matches!(ident.to_string().as_str(), "crate" | "self" | "super") => {}
            TokenTree::Ident(ident) if ident == "macro" => {
                let Some(TokenTree::Ident(_)) = tokens.next() else {
                    return None;
                };
                if matches!(tokens.peek(), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Parenthesis)
                {
                    tokens.next();
                }
                return matches!(tokens.next(), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Brace)
                    .then_some(ident.span());
            }
            TokenTree::Group(_) | TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => return None,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct OrderNode {
    pub(super) source_index: usize,
    pub(super) span: Span,
    pub(super) kind: ItemKind,
    pub(super) group: Option<ItemGroup>,
    pub(super) visibility: Option<VisibilityClass>,
    pub(super) label: String,
}

#[derive(Clone, Debug)]
pub(super) struct ModuleNode {
    pub(super) order: OrderNode,
    pub(super) indices: Vec<usize>,
}

#[derive(Clone, Debug)]
struct TypeCluster {
    type_index: usize,
    name: String,
    kind: ItemKind,
    visibility: VisibilityClass,
    inherent_impls: Vec<usize>,
    trait_impls: Vec<usize>,
}

pub(super) fn module_scopes(file: &syn::File) -> Vec<&[Item]> {
    let mut pending = VecDeque::from([file.items.as_slice()]);
    let mut scopes = Vec::new();

    while let Some(items) = pending.pop_front() {
        scopes.push(items);
        for item in items {
            if let Item::Mod(module) = item
                && let Some((_, nested_items)) = &module.content
            {
                pending.push_back(nested_items);
            }
        }
    }

    scopes
}

pub(super) fn module_nodes(items: &[Item]) -> Vec<ModuleNode> {
    let mut type_indices = HashMap::new();
    let mut clusters = HashMap::new();

    for (index, item) in items.iter().enumerate() {
        let Some((name, kind, visibility)) = self::type_definition(item) else {
            continue;
        };
        if type_indices.insert(name.clone(), Some(index)).is_some() {
            type_indices.insert(name, None);
            continue;
        }
        clusters.insert(
            index,
            TypeCluster {
                type_index: index,
                name,
                kind,
                visibility,
                inherent_impls: Vec::new(),
                trait_impls: Vec::new(),
            },
        );
    }

    for (index, item) in items.iter().enumerate() {
        let Item::Impl(item_impl) = item else {
            continue;
        };
        let Some(target_name) = self::impl_target_name(item_impl) else {
            continue;
        };
        let Some(Some(type_index)) = type_indices.get(&target_name) else {
            continue;
        };
        let Some(cluster) = clusters.get_mut(type_index) else {
            continue;
        };
        if item_impl.trait_.is_some() {
            cluster.trait_impls.push(index);
        } else {
            cluster.inherent_impls.push(index);
        }
    }

    let mut item_to_cluster = HashMap::new();
    for (&type_index, cluster) in &clusters {
        item_to_cluster.insert(type_index, type_index);
        for &index in cluster.inherent_impls.iter().chain(&cluster.trait_impls) {
            item_to_cluster.insert(index, type_index);
        }
    }

    let mut emitted_clusters = HashSet::new();
    let mut nodes = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(&type_index) = item_to_cluster.get(&index) {
            if !emitted_clusters.insert(type_index) {
                continue;
            }
            let Some(cluster) = clusters.get(&type_index) else {
                continue;
            };
            let Some(type_item) = items.get(cluster.type_index) else {
                continue;
            };
            let mut indices = vec![cluster.type_index];
            indices.extend(&cluster.inherent_impls);
            indices.extend(&cluster.trait_impls);
            let source_index = indices.iter().copied().min().unwrap_or(cluster.type_index);
            nodes.push(ModuleNode {
                order: OrderNode {
                    source_index,
                    span: self::item_span(type_item),
                    kind: cluster.kind,
                    group: Some(cluster.kind.group()),
                    visibility: Some(cluster.visibility),
                    label: format!("{} {}", cluster.kind.label(), cluster.name),
                },
                indices,
            });
        } else if let Some(classified) = self::classify_item(item) {
            nodes.push(ModuleNode {
                order: OrderNode {
                    source_index: index,
                    span: classified.span,
                    kind: classified.kind,
                    group: Some(classified.kind.group()),
                    visibility: item_visibility(item),
                    label: self::item_label(item, classified.kind),
                },
                indices: vec![index],
            });
        }
    }

    nodes.sort_unstable_by_key(|node| node.order.source_index);
    nodes
}

pub(super) fn impl_nodes(item_impl: &syn::ItemImpl) -> Vec<OrderNode> {
    item_impl
        .items
        .iter()
        .enumerate()
        .filter_map(|(source_index, item)| {
            let (kind, visibility) = match item {
                syn::ImplItem::Const(item) => (ItemKind::Const, Some(VisibilityClass::from(&item.vis))),
                syn::ImplItem::Fn(item) => (ItemKind::Fn, Some(VisibilityClass::from(&item.vis))),
                syn::ImplItem::Type(item) => (ItemKind::TypeAlias, Some(VisibilityClass::from(&item.vis))),
                syn::ImplItem::Macro(_) | syn::ImplItem::Verbatim(_) | _ => return None,
            };
            Some(OrderNode {
                source_index,
                span: self::impl_item_span(item),
                kind,
                group: None,
                visibility,
                label: self::impl_item_label(item, kind),
            })
        })
        .collect()
}

fn impl_item_label(item: &syn::ImplItem, kind: ItemKind) -> String {
    let name = match item {
        syn::ImplItem::Const(item) => Some(item.ident.to_string()),
        syn::ImplItem::Fn(item) => Some(item.sig.ident.to_string()),
        syn::ImplItem::Type(item) => Some(item.ident.to_string()),
        syn::ImplItem::Macro(_) | syn::ImplItem::Verbatim(_) | _ => None,
    };
    name.map_or_else(|| kind.label().to_owned(), |name| format!("{} {name}", kind.label()))
}

fn impl_item_span(item: &syn::ImplItem) -> Span {
    match item {
        syn::ImplItem::Const(item) => item.const_token.span,
        syn::ImplItem::Fn(item) => item.sig.fn_token.span,
        syn::ImplItem::Type(item) => item.type_token.span,
        syn::ImplItem::Macro(item) => item.mac.path.span(),
        syn::ImplItem::Verbatim(tokens) => tokens.span(),
        _ => item.span(),
    }
}
