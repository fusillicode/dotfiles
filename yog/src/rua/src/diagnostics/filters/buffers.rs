/// Filters out diagnostics based on the coded paths blacklist.
pub fn skip_diagnostics_for_buf_path(buf_path: &str) -> bool {
    paths_blacklist().iter().any(|up| buf_path.contains(up))
}

/// List of paths for which I don't want to report any diagnostic.
fn paths_blacklist() -> [String; 1] {
    let home_path = std::env::var("HOME").unwrap_or_default();
    [home_path + "/.cargo"]
}
