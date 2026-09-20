use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;

use syn::Item;

use super::import_resolution::ImportBinding;

pub struct ModuleScope<'ast> {
    pub path: Vec<String>,
    pub items: &'ast [Item],
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
        modules: HashSet::new(),
    };
    let mut pending = VecDeque::from([(Vec::new(), file.items.as_slice())]);

    while let Some((path, items)) = pending.pop_front() {
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
                if let Some((_, nested_items)) = &module.content {
                    pending.push_back((nested_path, nested_items.as_slice()));
                }
            }
            if let Item::Use(item_use) = item {
                let bindings = super::import_resolution::use_bindings(&item_use.tree, &path);
                info.imports.extend(bindings.bindings);
                info.glob_imports.extend(bindings.glob_imports);
                info.unknown_imports |= bindings.unknown;
            }
        }
        idx.info.insert(path.clone(), info);
        idx.scopes.push(ModuleScope { path, items });
    }

    idx
}

fn item_name(item: &Item) -> Option<String> {
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

pub fn is_local_module_path(idx: &ModuleIdx<'_>, path: &[String]) -> bool {
    path.is_empty() || idx.modules.contains(path)
}
