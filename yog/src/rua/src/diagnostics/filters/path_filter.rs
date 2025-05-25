use mlua::prelude::*;

/// Filters out diagnostics related to the `unwanted_paths`.
pub fn no_diagnostics_for_path(lua: &Lua, buf_path: &str) -> Option<LuaResult<LuaTable>> {
    if unwanted_paths().iter().any(|up| buf_path.contains(up)) {
        return Some(lua.create_sequence_from::<LuaTable>(vec![]));
    }
    None
}

/// List of paths for which I don't want to report any diagnostic.
fn unwanted_paths() -> [String; 1] {
    let home_path = std::env::var("HOME").unwrap_or_default();
    [home_path + "/.cargo"]
}
