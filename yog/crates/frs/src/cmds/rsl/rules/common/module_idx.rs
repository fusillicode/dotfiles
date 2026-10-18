use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use proc_macro2::LineColumn;
use syn::Item;
use syn::spanned::Spanned;

use super::import_resolution::ImportBinding;

pub struct ModuleScope<'ast> {
    pub path: Vec<String>,
    pub items: &'ast [Item],
    pub body_start: LineColumn,
}

#[derive(Default)]
pub struct ScopeInfo {
    pub definitions: HashSet<String>,
    pub fns: HashSet<String>,
    pub glob_imports: Vec<Vec<String>>,
    pub imports: Vec<ImportBinding>,
    pub unknown_imports: bool,
}

pub struct ModuleIdx<'ast> {
    pub scopes: Vec<ModuleScope<'ast>>,
    pub info: HashMap<Vec<String>, ScopeInfo>,
    pub modules: HashSet<Vec<String>>,
}

pub fn module_idx(file: &syn::File) -> ModuleIdx<'_> {
    let mut idx = ModuleIdx {
        scopes: Vec::new(),
        info: HashMap::new(),
        modules: HashSet::from([Vec::new()]),
    };

    let body_start = file
        .attrs
        .last()
        .map_or(LineColumn { line: 1, column: 0 }, |attr| attr.span().end());
    let mut pending = VecDeque::from([(Vec::new(), file.items.as_slice(), body_start)]);

    while let Some((path, items, body_start)) = pending.pop_front() {
        let mut info = ScopeInfo::default();

        for item in items {
            if let Some(name) = self::item_name(item) {
                info.definitions.insert(name);
            }

            if let Item::Fn(fn_item) = item {
                info.fns.insert(fn_item.sig.ident.to_string());
            }

            if let Item::Mod(module) = item {
                let mut nested_path = path.clone();
                nested_path.push(module.ident.to_string());
                idx.modules.insert(nested_path.clone());

                if let Some((brace, nested_items)) = &module.content {
                    let body_start = module
                        .attrs
                        .iter()
                        .rfind(|attr| matches!(attr.style, syn::AttrStyle::Inner(_)))
                        .map_or_else(|| brace.span.open().end(), |attr| attr.span().end());
                    pending.push_back((nested_path, nested_items.as_slice(), body_start));
                }
            }

            if let Item::Use(item_use) = item {
                if item_use.leading_colon.is_some() || super::has_conditional_attributes(&item_use.attrs) {
                    info.unknown_imports = true;
                    continue;
                }

                let bindings = super::import_resolution::use_bindings(&item_use.tree, &path);
                info.imports.extend(bindings.bindings);
                info.glob_imports.extend(bindings.glob_imports);
                info.unknown_imports |= bindings.unknown;
            }
        }
        idx.info.insert(path.clone(), info);
        idx.scopes.push(ModuleScope {
            path,
            items,
            body_start,
        });
    }

    idx
}

pub fn is_local_module_path(idx: &ModuleIdx<'_>, path: &[String]) -> bool {
    path.is_empty() || idx.modules.contains(path)
}

pub(super) fn item_name(item: &Item) -> Option<String> {
    match item {
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
        Item::ForeignMod(_) | Item::Impl(_) | Item::Macro(_) | Item::Use(_) | Item::Verbatim(_) | _ => None,
    }
}

pub(in crate::cmds::rsl::rules) fn has_external_modules(items: &[Item]) -> bool {
    let mut pending = vec![items];

    while let Some(items) = pending.pop() {
        for item in items {
            let Item::Mod(module) = item else {
                continue;
            };

            let Some((_, items)) = &module.content else {
                return true;
            };

            pending.push(items);
        }
    }

    false
}
