//! Shared syntax classification helpers for the rsl rules.

use proc_macro2::TokenTree;
use syn::Item;

#[derive(Clone, Copy, Debug, strum::Display, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, strum::Display, Eq, Ord, PartialEq, PartialOrd)]
pub enum VisibilityClass {
    #[strum(to_string = "pub")]
    Public,
    #[strum(to_string = "pub(crate)")]
    Crate,
    #[strum(to_string = "restricted")]
    Restricted,
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

pub fn type_definition_name(item: &Item) -> Option<String> {
    match item {
        Item::Enum(item) => Some(item.ident.to_string()),
        Item::Struct(item) => Some(item.ident.to_string()),
        Item::Union(item) => Some(item.ident.to_string()),
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
    if let Item::Impl(item) = item {
        let target = self::impl_target_name(item).unwrap_or_else(|| "type".to_owned());
        return item.trait_.as_ref().map_or_else(
            || format!("impl {target}"),
            |(path, _)| {
                let name = path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");
                format!("impl {name} for {target}")
            },
        );
    }
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
        Item::Macro(item) => item.ident.as_ref().map(ToString::to_string),
        Item::Use(_) | Item::ForeignMod(_) | Item::Impl(_) | Item::Verbatim(_) | _ => None,
    };
    name.map_or_else(|| kind.label().to_owned(), |name| format!("{} {name}", kind.label()))
}

pub fn classify_item(item: &Item) -> Option<ItemKind> {
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
        Item::Verbatim(tokens) if self::is_explicit_macro(tokens) => ItemKind::Macro,
        Item::Macro(_) | Item::Verbatim(_) | _ => return None,
    };

    Some(kind)
}

pub(super) fn is_test_module_declaration(module: &syn::ItemMod) -> bool {
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

fn is_explicit_macro(tokens: &proc_macro2::TokenStream) -> bool {
    let mut tokens = tokens.clone().into_iter().peekable();
    loop {
        let Some(token) = tokens.next() else {
            return false;
        };
        match token {
            TokenTree::Punct(punct) if punct.as_char() == '#' => {
                if !matches!(tokens.next(), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Bracket)
                {
                    return false;
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
                    return false;
                };
                if matches!(tokens.peek(), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Parenthesis)
                {
                    tokens.next();
                }
                return matches!(tokens.next(), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Brace);
            }
            TokenTree::Group(_) | TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => return false,
        }
    }
}
