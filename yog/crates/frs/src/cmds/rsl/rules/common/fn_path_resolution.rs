use std::collections::HashSet;

use super::import_resolution::imported_binding;
use super::module_idx::ModuleIdx;
use super::module_idx::ScopeInfo;
use super::module_idx::is_local_module_path;
use super::path_resolution::is_associated_fn_path;
use super::path_resolution::is_non_fn_call_path;
use super::path_resolution::normalize_path;
use super::path_resolution::path_parts;

pub struct FnCallSuggestion {
    pub expected_path: String,
    pub required_import: Option<String>,
}

pub fn expected_fn_path(
    idx: &ModuleIdx<'_>,
    current_module: &[String],
    local_bindings: &[HashSet<String>],
    path: &syn::Path,
) -> Option<FnCallSuggestion> {
    let parts = path_parts(path)?;
    if is_non_fn_call_path(&parts) || is_associated_fn_path(&parts) {
        return None;
    }

    if parts.len() == 1 && parts.first().is_some_and(|name| is_local_binding(local_bindings, name)) {
        return None;
    }

    if local_fn(idx, current_module, path).is_some() && fn_path_tail(&parts).len() <= 2 {
        return None;
    }

    let name = parts.last()?;
    if parts.len() == 1 {
        // TODO: Resolve bare calls whose definitions are outside the current file.
        return imported_fn_path(idx, current_module, name);
    }

    let qualified = fn_path_tail(&parts);
    if qualified.len() <= 2 || !can_use_shortened_module(idx, current_module, &parts) {
        return None;
    }

    let expected_path = qualified
        .get(qualified.len().saturating_sub(2)..)
        .map(|parts| parts.join("::"))?;
    let fn_idx = parts.len().saturating_sub(1);
    let target_module = normalize_path(current_module, parts.get(..fn_idx).unwrap_or_default());

    Some(FnCallSuggestion {
        expected_path,
        required_import: required_module_import(idx, current_module, &parts, &target_module),
    })
}

fn is_local_binding(local_bindings: &[HashSet<String>], name: &str) -> bool {
    local_bindings.iter().rev().any(|bindings| bindings.contains(name))
}

fn imported_fn_path(idx: &ModuleIdx<'_>, current_module: &[String], name: &str) -> Option<FnCallSuggestion> {
    let binding = imported_binding(idx, current_module, name)?;
    let source_path = &binding.path;
    if !can_use_imported_module(idx, current_module, source_path) {
        return None;
    }

    let expected_path = (source_path.len() >= 2)
        .then(|| source_path.get(source_path.len().saturating_sub(2)..))
        .flatten()
        .map(|parts| parts.join("::"))?;
    let fn_idx = source_path.len().saturating_sub(1);
    let target_module = source_path.get(..fn_idx)?;

    Some(FnCallSuggestion {
        expected_path,
        required_import: required_module_import(idx, current_module, &binding.source_path, target_module),
    })
}

fn required_module_import(
    idx: &ModuleIdx<'_>,
    current_module: &[String],
    source_path: &[String],
    target_module: &[String],
) -> Option<String> {
    let module_name = target_module.last()?;
    if module_is_available(idx, current_module, module_name, target_module) {
        return None;
    }

    let source_module_path = source_path.get(..source_path.len().saturating_sub(1))?;
    let import_path = match source_module_path.first().map(String::as_str) {
        Some("crate" | "self" | "super") => crate_path(target_module),
        Some(_) if source_module_path.len() == 1 && !idx.modules.contains(target_module) => return None,
        Some(_) if idx.modules.contains(target_module) => crate_path(target_module),
        Some(_) => source_module_path.join("::"),
        None => return None,
    };

    Some(format!("use {import_path};"))
}

fn module_is_available(
    idx: &ModuleIdx<'_>,
    current_module: &[String],
    module_name: &str,
    target_module: &[String],
) -> bool {
    let Some(scope) = idx.info.get(current_module) else {
        return false;
    };
    if scope
        .imports
        .iter()
        .any(|binding| binding.name == module_name && binding.path == target_module)
    {
        return true;
    }

    let mut local_module = current_module.to_vec();
    local_module.push(module_name.to_owned());
    scope.definitions.contains(module_name) && local_module == target_module && idx.modules.contains(&local_module)
}

fn crate_path(parts: &[String]) -> String {
    std::iter::once("crate".to_owned())
        .chain(parts.iter().cloned())
        .collect::<Vec<_>>()
        .join("::")
}

fn can_use_shortened_module(idx: &ModuleIdx<'_>, current_module: &[String], parts: &[String]) -> bool {
    let Some(fn_idx) = parts.len().checked_sub(1) else {
        return false;
    };
    let Some(module_name) = parts.get(fn_idx.saturating_sub(1)) else {
        return false;
    };
    let target_module = normalize_path(current_module, parts.get(..fn_idx).unwrap_or_default());

    can_use_module_name(idx, current_module, module_name, &target_module)
}

fn can_use_imported_module(idx: &ModuleIdx<'_>, current_module: &[String], source_path: &[String]) -> bool {
    let Some(fn_idx) = source_path.len().checked_sub(1) else {
        return false;
    };
    let Some(module_name) = source_path.get(fn_idx.saturating_sub(1)) else {
        return false;
    };
    let target_module = source_path.get(..fn_idx).unwrap_or_default();

    // The explicit import resolves the callable; defer only the module-name conflict to future glob resolution.
    if idx.info.get(current_module).is_some_and(|scope| scope.unknown_imports) {
        return true;
    }

    can_use_module_name(idx, current_module, module_name, target_module)
}

fn can_use_module_name(idx: &ModuleIdx<'_>, current_module: &[String], name: &str, target_module: &[String]) -> bool {
    let Some(scope) = idx.info.get(current_module) else {
        return false;
    };
    // TODO: Resolve glob imports before deciding whether the shortened module name conflicts.
    if scope.unknown_imports {
        return false;
    }

    let mut local_module = current_module.to_vec();
    local_module.push(name.to_owned());
    if scope.definitions.contains(name) && (!idx.modules.contains(&local_module) || local_module != target_module) {
        return false;
    }

    scope
        .imports
        .iter()
        .filter(|binding| binding.name == name)
        .all(|binding| binding.path == target_module)
}

fn fn_path_tail(parts: &[String]) -> &[String] {
    let mut first = 0;
    while matches!(parts.get(first).map(String::as_str), Some("crate" | "self" | "super")) {
        first = first.saturating_add(1);
    }
    parts.get(first..).unwrap_or_default()
}

fn local_fn(idx: &ModuleIdx<'_>, current_module: &[String], path: &syn::Path) -> Option<(Vec<String>, String)> {
    let parts = path_parts(path)?;
    let name = parts.last()?.clone();
    let last_item = parts.len().saturating_sub(1);
    let mut module_path = current_module.to_vec();
    let mut first_item = 0;

    match parts.first()?.as_str() {
        "crate" => {
            module_path.clear();
            first_item = 1;
        }
        "self" => first_item = 1,
        "super" => {
            while parts.get(first_item).is_some_and(|part| part == "super") {
                module_path.pop()?;
                first_item = first_item.saturating_add(1);
            }
        }
        _ => {}
    }

    if first_item > last_item {
        return None;
    }
    module_path.extend(parts.get(first_item..last_item)?.iter().cloned());
    if !is_local_module_path(idx, &module_path) {
        return None;
    }

    let scope = idx.info.get(&module_path)?;
    if scope.imports.iter().any(|binding| binding.name == name) {
        return None;
    }
    if scope.fns.contains(&name) {
        return Some((module_path, name));
    }
    if let Some(glob_module) = glob_imported_fn(idx, scope, &name) {
        return Some((glob_module, name));
    }
    if scope.unknown_imports {
        return None;
    }
    None
}

fn glob_imported_fn(idx: &ModuleIdx<'_>, scope: &ScopeInfo, name: &str) -> Option<Vec<String>> {
    let mut candidate = None;
    for glob_module in &scope.glob_imports {
        let Some(glob_scope) = idx.info.get(glob_module) else {
            // TODO: Resolve glob imports from modules outside this file.
            continue;
        };
        if !glob_scope.fns.contains(name) {
            continue;
        }
        if candidate.is_some() {
            // TODO: Resolve ambiguous names imported from multiple glob sources.
            return None;
        }
        candidate = Some(glob_module.clone());
    }
    candidate
}
