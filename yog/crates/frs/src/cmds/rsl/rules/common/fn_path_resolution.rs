use super::import_resolution::imported_binding;
use super::module_idx::ModuleIdx;
use super::module_idx::ScopeInfo;
use super::module_idx::is_local_module_path;
use super::path_resolution::is_associated_fn_path;
use super::path_resolution::is_non_fn_call_path;
use super::path_resolution::normalize_path;
use super::path_resolution::path_parts;
use crate::cmds::rsl::engine::FileContext;

pub struct FnCallSuggestion {
    pub expected_path: String,
    pub target: CallTarget,
}

#[derive(Clone)]
pub enum CallTarget {
    SameModule,
    Module { path: Vec<String>, access: ModuleAccess },
}

#[derive(Clone)]
pub enum ModuleAccess {
    Local,
    Declared,
    Imported(usize),
    Import(String),
    ExternalCrate,
    Anchored,
}

pub fn expected_fn_path(
    ctx: &FileContext<'_>,
    idx: &ModuleIdx<'_>,
    current_module: &[String],
    path: &syn::Path,
) -> Option<FnCallSuggestion> {
    let parts = path_parts(path)?;

    if is_non_fn_call_path(&parts) || is_associated_fn_path(&parts) {
        return None;
    }

    if is_same_module_fn(ctx, idx, current_module, &parts) {
        return Some(FnCallSuggestion {
            expected_path: parts.last()?.clone(),
            target: CallTarget::SameModule,
        });
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

    if qualified.len() <= 2 {
        return None;
    }

    let expected_path = qualified
        .get(qualified.len().saturating_sub(2)..)
        .map(|parts| parts.join("::"))?;
    let fn_idx = parts.len().saturating_sub(1);
    let target_module = normalize_path(current_module, parts.get(..fn_idx).unwrap_or_default());

    Some(FnCallSuggestion {
        expected_path,
        target: CallTarget::Module {
            access: module_access(idx, current_module, &parts, &target_module)?,
            path: target_module,
        },
    })
}

fn is_same_module_fn(ctx: &FileContext<'_>, idx: &ModuleIdx<'_>, current_module: &[String], parts: &[String]) -> bool {
    let Some((name, module)) = parts.split_last() else {
        return false;
    };

    if !idx
        .info
        .get(current_module)
        .is_some_and(|scope| scope.fns.contains(name))
    {
        return false;
    }

    match module.first().map(String::as_str) {
        Some("self") => module.len() == 1,
        Some("crate") => ctx
            .modules
            .anchor(ctx.path, current_module, 0)
            .is_ok_and(|current| current == module.join("::")),
        Some(_) | None => false,
    }
}

fn imported_fn_path(idx: &ModuleIdx<'_>, current_module: &[String], name: &str) -> Option<FnCallSuggestion> {
    let binding = imported_binding(idx, current_module, name)?;
    let source_path = &binding.path;
    let source_module = binding.source_path.get(..binding.source_path.len().saturating_sub(1))?;

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
        target: CallTarget::Module {
            access: module_access(idx, current_module, &binding.source_path, target_module)?,
            path: target_module.to_vec(),
        },
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

    if let Some(access) = available_module(idx, current_module, module_name, target_module) {
        return Some(access);
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

fn available_module(
    idx: &ModuleIdx<'_>,
    current_module: &[String],
    module_name: &str,
    target_module: &[String],
) -> Option<ModuleAccess> {
    let scope = idx.info.get(current_module)?;
    let mut local_module = current_module.to_vec();
    local_module.push(module_name.to_owned());
    if scope.definitions.contains(module_name) && local_module == target_module && idx.modules.contains(&local_module) {
        return Some(ModuleAccess::Declared);
    }
    scope
        .imports
        .iter()
        .position(|binding| binding.name == module_name && binding.path == target_module)
        .map(ModuleAccess::Imported)
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
