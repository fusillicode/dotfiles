use syn::Path;
use syn::PathArguments;

// Named primitive types cannot serve as module roots in generated imports.
// Keep this list in sync with Rust's primitive types, including unstable named types.
// Compound primitives such as tuples and arrays have no path name to classify here.
const PRIMITIVE_TYPE_NAMES: &[&str] = &[
    "bool", "char", "str", // Boolean and text types.
    "f16", "f32", "f64", "f128", // Floating-point types.
    "i8", "i16", "i32", "i64", "i128", "isize", // Signed integers.
    "u8", "u16", "u32", "u64", "u128", "usize", // Unsigned integers.
];

pub fn path_parts(path: &Path) -> Option<Vec<String>> {
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

pub fn is_associated_fn_path(parts: &[String]) -> bool {
    has_primitive_type_root(parts)
        || parts
            .get(..parts.len().saturating_sub(1))
            .is_some_and(|prefix| prefix.iter().any(|part| is_type_name(part)))
}

pub fn is_non_fn_call_path(parts: &[String]) -> bool {
    parts.last().is_some_and(|name| is_type_name(name))
}

pub fn is_import_style_path(parts: &[String]) -> bool {
    !has_primitive_type_root(parts)
        && parts
            .first()
            .is_some_and(|part| matches!(part.as_str(), "crate" | "self" | "super") || is_module_name(part))
}

pub fn normalize_path(current_module: &[String], parts: &[String]) -> Vec<String> {
    let mut normalized = current_module.to_vec();
    let mut first_item = 0;

    match parts.first().map(String::as_str) {
        Some("crate") => {
            // The supplied file's position in the crate is unknown.
            normalized = vec!["crate".to_owned()];
            first_item = 1;
        }
        Some("self") => first_item = 1,
        Some("super") => {
            while parts.get(first_item).is_some_and(|part| part == "super") {
                if normalized.last().is_none_or(|part| part == "super") {
                    normalized.push("super".to_owned());
                } else {
                    normalized.pop();
                }
                first_item = first_item.saturating_add(1);
            }
        }
        Some(_) | None => {}
    }

    if let Some(suffix) = parts.get(first_item..) {
        normalized.extend(suffix.iter().cloned());
    }
    normalized
}

pub fn path_label(path: &Path) -> String {
    path_parts(path).map_or_else(|| "qualified path".to_owned(), |parts| parts.join("::"))
}

fn has_primitive_type_root(parts: &[String]) -> bool {
    let primitive = match parts {
        [root, module, name, ..] if matches!(root.as_str(), "std" | "core") && module == "primitive" => name,
        [name, ..] => name,
        [] => return false,
    };

    // std::str and std::i64 are modules; std::primitive::str and bare str are types.
    PRIMITIVE_TYPE_NAMES.contains(&primitive.as_str())
}

fn is_module_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_lowercase)
}

fn is_type_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}
