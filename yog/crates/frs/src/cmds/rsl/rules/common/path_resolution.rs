use syn::PathArguments;

pub fn path_parts(path: &syn::Path) -> Option<Vec<String>> {
    if path.leading_colon.is_some() || path.segments.is_empty() {
        return None;
    }

    let last_segment = path.segments.len().saturating_sub(1);
    let mut parts = Vec::new();
    for (idx, segment) in path.segments.iter().enumerate() {
        if idx != last_segment && !matches!(&segment.arguments, PathArguments::None) {
            return None;
        }
        parts.push(segment.ident.to_string());
    }
    Some(parts)
}

pub fn associated_receiver_parts(path: &syn::Path) -> Option<Vec<String>> {
    let parts = self::path_parts(path)?;
    is_associated_fn_path(&parts)
        .then(|| parts.get(..parts.len().saturating_sub(1)))
        .flatten()
        .map(ToOwned::to_owned)
}

pub fn is_associated_fn_path(parts: &[String]) -> bool {
    parts
        .get(..parts.len().saturating_sub(1))
        .is_some_and(|prefix| prefix.iter().any(|part| is_type_name(part)))
}

pub fn is_non_fn_call_path(parts: &[String]) -> bool {
    parts.last().is_some_and(|name| is_type_name(name))
}

pub fn is_import_style_path(parts: &[String]) -> bool {
    parts
        .first()
        .is_some_and(|part| matches!(part.as_str(), "crate" | "self" | "super") || is_module_name(part))
}

fn is_module_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

fn is_type_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

pub fn normalize_path(current_module: &[String], parts: &[String]) -> Vec<String> {
    let mut absolute_parts = current_module.to_vec();
    let mut first_item = 0;

    match parts.first().map(String::as_str) {
        Some("crate") => {
            absolute_parts.clear();
            first_item = 1;
        }
        Some("self") => first_item = 1,
        Some("super") => {
            while parts.get(first_item).is_some_and(|part| part == "super") {
                if absolute_parts.pop().is_none() {
                    return Vec::new();
                }
                first_item = first_item.saturating_add(1);
            }
        }
        Some(_) | None => {}
    }

    if let Some(suffix) = parts.get(first_item..) {
        absolute_parts.extend(suffix.iter().cloned());
    }
    absolute_parts
}

pub fn path_label(path: &syn::Path) -> String {
    self::path_parts(path).map_or_else(|| "qualified path".to_owned(), |parts| parts.join("::"))
}
