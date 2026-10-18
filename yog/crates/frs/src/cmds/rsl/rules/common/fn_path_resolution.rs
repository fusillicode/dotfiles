use super::fn_call_analysis::module_type_names;
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
    pub target_module: Vec<String>,
    pub module: ModuleAccess,
}

pub enum ModuleAccess {
    InScope,
    Import(String),
    ExternalCrate,
    Anchored,
}

pub fn expected_fn_path(idx: &ModuleIdx<'_>, current_module: &[String], path: &syn::Path) -> Option<FnCallSuggestion> {
    let parts = path_parts(path)?;

    if is_non_fn_call_path(&parts) || is_associated_fn_path(&parts) {
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
        module: module_access(idx, current_module, &parts, &target_module)?,
        target_module,
    })
}

fn imported_fn_path(idx: &ModuleIdx<'_>, current_module: &[String], name: &str) -> Option<FnCallSuggestion> {
    let binding = imported_binding(idx, current_module, name)?;
    let source_path = &binding.path;
    let source_module = binding.source_path.get(..binding.source_path.len().saturating_sub(1))?;

    if !is_module_root(source_module) && !can_use_imported_module(idx, current_module, source_path) {
        return None;
    }

    let fn_idx = source_path.len().saturating_sub(1);
    let target_module = source_path.get(..fn_idx)?;
    let expected_path = if is_module_root(source_module) {
        binding.source_path.join("::")
    } else {
        (source_path.len() >= 2)
            .then(|| source_path.get(source_path.len().saturating_sub(2)..))
            .flatten()
            .map(|parts| parts.join("::"))?
    };

    Some(FnCallSuggestion {
        expected_path,
        module: module_access(idx, current_module, &binding.source_path, target_module)?,
        target_module: target_module.to_vec(),
    })
}

fn module_access(
    idx: &ModuleIdx<'_>,
    current_module: &[String],
    source_path: &[String],
    target_module: &[String],
) -> Option<ModuleAccess> {
    let source_module_path = source_path.get(..source_path.len().saturating_sub(1))?;
    if is_module_root(source_module_path) {
        return Some(ModuleAccess::Anchored);
    }
    let module_name = target_module.last()?;

    if module_is_available(idx, current_module, module_name, target_module) {
        return Some(ModuleAccess::InScope);
    }

    let import_path = match source_module_path.first().map(String::as_str) {
        Some("crate" | "self" | "super") => source_module_path.join("::"),
        Some(_) if source_module_path.len() == 1 && !idx.modules.contains(target_module) => {
            return Some(ModuleAccess::ExternalCrate);
        }
        Some(_) if idx.modules.contains(target_module) => format!("self::{}", source_module_path.join("::")),
        Some(_) => source_module_path.join("::"),
        None => return None,
    };

    // Retain crate:: and super:: anchors verbatim. Moving that prefix to a use
    // in the same module does not require discovering the file's crate position.
    Some(ModuleAccess::Import(import_path))
}

fn is_module_root(parts: &[String]) -> bool {
    !parts.is_empty()
        && parts
            .iter()
            .all(|part| matches!(part.as_str(), "crate" | "self" | "super"))
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

    let type_definition = idx
        .scopes
        .iter()
        .find(|scope| scope.path == current_module)
        .is_some_and(|scope| module_type_names(scope.items).contains(name));
    if type_definition && (!idx.modules.contains(&local_module) || local_module != target_module) {
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
        "crate" => return None,
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
