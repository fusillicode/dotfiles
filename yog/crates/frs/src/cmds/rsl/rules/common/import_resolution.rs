use syn::UseTree;

use super::module_idx::ModuleIdx;
use super::module_idx::ScopeInfo;
use super::path_resolution::normalize_path;

pub struct ImportBinding {
    pub name: String,
    pub path: Vec<String>,
    pub source_path: Vec<String>,
}

#[derive(Default)]
pub struct UseBindings {
    pub bindings: Vec<ImportBinding>,
    pub glob_imports: Vec<Vec<String>>,
    pub unknown: bool,
}

pub fn use_bindings(tree: &UseTree, current_module: &[String]) -> UseBindings {
    let mut bindings = UseBindings::default();
    let mut pending = vec![(tree, Vec::<String>::new())];

    while let Some((tree, prefix)) = pending.pop() {
        match tree {
            UseTree::Path(path) => {
                let mut next_prefix = prefix;
                next_prefix.push(path.ident.to_string());
                pending.push((path.tree.as_ref(), next_prefix));
            }
            UseTree::Group(group) => {
                pending.extend(group.items.iter().map(|tree| (tree, prefix.clone())));
            }
            UseTree::Name(name) => {
                if name.ident == "self" {
                    if let Some(binding) = prefix.last() {
                        bindings.bindings.push(ImportBinding {
                            name: binding.clone(),
                            path: normalize_path(current_module, &prefix),
                            source_path: prefix,
                        });
                    }
                } else {
                    let mut path = prefix;
                    path.push(name.ident.to_string());
                    bindings.bindings.push(ImportBinding {
                        name: name.ident.to_string(),
                        path: normalize_path(current_module, &path),
                        source_path: path,
                    });
                }
            }
            UseTree::Rename(rename) => {
                if rename.rename != "_" {
                    let mut path = prefix;
                    path.push(rename.ident.to_string());
                    bindings.bindings.push(ImportBinding {
                        name: rename.rename.to_string(),
                        path: normalize_path(current_module, &path),
                        source_path: path,
                    });
                }
            }
            UseTree::Glob(_) => {
                bindings.glob_imports.push(normalize_path(current_module, &prefix));
                bindings.unknown = true;
            }
        }
    }

    bindings
}

pub fn has_name_clash_parts(idx: &ModuleIdx<'_>, current_module: &[String], parts: &[String]) -> bool {
    let Some(scope) = idx.info.get(current_module) else {
        return true;
    };
    if scope.unknown_imports {
        return true;
    }

    let Some(name) = parts.last() else {
        return true;
    };
    let Some(target_module_parts) = parts.get(..parts.len().saturating_sub(1)) else {
        return true;
    };
    let target_module = normalize_path(current_module, target_module_parts);
    let target_path = normalize_path(current_module, parts);

    if target_module != current_module && scope.definitions.contains(name) {
        return true;
    }

    scope
        .imports
        .iter()
        .any(|binding| binding.name == *name && binding.path != target_path)
}

pub fn imported_binding<'idx>(
    idx: &'idx ModuleIdx<'_>,
    current_module: &[String],
    name: &str,
) -> Option<&'idx ImportBinding> {
    let scope = idx.info.get(current_module)?;
    if let Some(binding) = self::direct_imported_binding(scope, name) {
        return Some(binding);
    }

    let mut candidate = None;
    for glob_module in &scope.glob_imports {
        let Some(glob_scope) = idx.info.get(glob_module) else {
            // TODO: Resolve explicit imports exported by modules outside this file.
            continue;
        };
        let Some(binding) = self::direct_imported_binding(glob_scope, name) else {
            continue;
        };
        if candidate.is_some() {
            // TODO: Resolve ambiguous explicit imports exported by multiple glob sources.
            return None;
        }
        candidate = Some(binding);
    }
    candidate
}

fn direct_imported_binding<'idx>(scope: &'idx ScopeInfo, name: &str) -> Option<&'idx ImportBinding> {
    let mut bindings = scope.imports.iter().filter(|binding| binding.name == name);
    let binding = bindings.next()?;
    bindings.next().is_none().then_some(binding)
}
