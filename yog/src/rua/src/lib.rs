use mlua::chunk;
use mlua::prelude::*;

#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("format_diagnostic", lua.create_function(format_diagnostic)?)?;
    exports.set(
        "filter_diagnostics",
        lua.create_function(filter_diagnostics)?,
    )?;
    Ok(exports)
}

fn format_diagnostic(_lua: &Lua, lsp_diag: LuaTable) -> LuaResult<String> {
    let lsp_data = dig::<LuaTable>(&lsp_diag, &["user_data", "lsp"])?;

    let diag_msg = dig::<String>(&lsp_data, &["data", "rendered"])
        .or_else(|_| dig::<String>(&lsp_data, &["message"]))
        .map(|s| s.trim_end_matches('.').to_owned())?;

    let src_and_code = match (
        dig::<String>(&lsp_data, &["source"])
            .map(|s| s.trim_end_matches('.').to_owned())
            .ok(),
        dig::<String>(&lsp_data, &["code"])
            .map(|s| s.trim_end_matches('.').to_owned())
            .ok(),
    ) {
        (None, None) => None,
        (Some(src), None) => Some(src),
        (None, Some(code)) => Some(code),
        (Some(src), Some(code)) => Some(format!("{src}: {code}")),
    }
    .map(|s| format!(" [{s}]"))
    .unwrap_or_else(String::new);

    Ok(format!("▶ {diag_msg}{src_and_code}"))
}

fn filter_diagnostics(lua: &Lua, lsp_diags: LuaTable) -> LuaResult<LuaTable> {
    let filtered = lua.create_table()?;
    for lsp_diag in lsp_diags.sequence_values::<LuaTable>().flatten() {
        let Ok(rel_infos) = dig::<LuaTable>(&lsp_diag, &["user_data", "lsp", "relatedInformation"])
        else {
            continue;
        };
        for rel_info in rel_infos.sequence_values::<LuaTable>().flatten() {
            let start = dig::<LuaTable>(&rel_info, &["location", "range", "start"])?;
            let start_line = dig::<usize>(&start, &["line"])?;
            let start_col = dig::<usize>(&start, &["character"])?;
            let end = dig::<LuaTable>(&rel_info, &["location", "range", "end"])?;
            let end_line = dig::<usize>(&end, &["line"])?;
            let end_col = dig::<usize>(&end, &["character"])?;
            ndbg(lua, start_line).unwrap();
            ndbg(lua, start_col).unwrap();
            ndbg(lua, end_line).unwrap();
            ndbg(lua, end_col).unwrap();
        }
        filtered.push(lsp_diag)?;
    }
    Ok(filtered)
}

fn dig<T: FromLua>(tbl: &LuaTable, keys: &[&str]) -> Result<T, DigError> {
    match keys {
        [] => Err(DigError::NoKeysSupplied),
        [leaf] => tbl.raw_get::<T>(*leaf).map_err(DigError::ConversionError),
        [path @ .., leaf] => {
            let mut tbl = tbl.to_owned();
            for key in path {
                tbl = tbl
                    .raw_get::<LuaTable>(*key)
                    .map_err(|e| DigError::KeyNotFound(key.to_string(), e))?;
            }
            tbl.raw_get::<T>(*leaf).map_err(DigError::ConversionError)
        }
    }
}

#[derive(thiserror::Error, Debug)]
enum DigError {
    #[error("no keys supplied")]
    NoKeysSupplied,
    #[error("key {0:?} not found, error {1:?}")]
    KeyNotFound(String, mlua::Error),
    #[error("type conversion error {0:?}")]
    ConversionError(mlua::Error),
}

impl From<DigError> for mlua::Error {
    fn from(value: DigError) -> Self {
        mlua::Error::external(value)
    }
}

#[allow(dead_code)]
fn ndbg<T: mlua::IntoLuaMulti>(lua: &Lua, value: T) -> mlua::Result<()> {
    lua.load(chunk! { return function(tbl) print(vim.inspect(tbl)) end })
        .eval::<mlua::Function>()?
        .call::<()>(value)
}
