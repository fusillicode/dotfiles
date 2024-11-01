use mlua::prelude::*;

#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("format_diagnostic", lua.create_function(format_diagnostic)?)?;
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
    #[error("key {0:?} not found {1:?}")]
    KeyNotFound(String, mlua::Error),
    #[error("cannot convert to supplied type {0:?}")]
    ConversionError(mlua::Error),
}

impl From<DigError> for mlua::Error {
    fn from(value: DigError) -> Self {
        mlua::Error::external(value)
    }
}

#[allow(dead_code)]
fn ndbg<'a, T: std::fmt::Debug>(lua: &Lua, value: &'a T) -> &'a T {
    let print = lua.globals().get::<LuaFunction>("print").unwrap();
    // TODO: try to understand how to print inspect
    // let inspect = lua.globals().get::<LuaTable>("vim").unwrap();
    print.call::<()>(format!("{value:?}")).unwrap();
    value
}
