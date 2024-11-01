use mlua::prelude::*;

#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("format_diagnostic", lua.create_function(format_diagnostic)?)?;
    Ok(exports)
}

// local function format_diagnostic(diagnostic)
//   local message =
//       (
//         vim.tbl_get(diagnostic, 'user_data', 'lsp', 'data', 'rendered') or
//         vim.tbl_get(diagnostic, 'user_data', 'lsp', 'message') or
//         ''
//       ):gsub('%.$', '')
//   if message == '' then return end
//
//   local lsp_data = vim.tbl_get(diagnostic, 'user_data', 'lsp')
//   if not lsp_data then return end
//
//   local source_code_tbl = vim.tbl_filter(function(x) return x ~= nil and x ~= '' end, {
//     lsp_data.source and lsp_data.source:gsub('%.$', '') or nil,
//     lsp_data.code and lsp_data.code:gsub('%.$', '') or nil,
//   })
//   local source_code = table.concat(source_code_tbl, ': ')
//
//   return '▶ ' .. message .. (source_code ~= '' and ' [' .. source_code .. ']' or '')
// end
fn format_diagnostic(lua: &Lua, lsp_diagnostic: LuaTable) -> LuaResult<String> {
    let rendered = dig::<String>(&lsp_diagnostic, &["user_data", "lsp", "data", "rendered"]);
    let message = dig::<String>(&lsp_diagnostic, &["user_data", "lsp", "data", "message"]);

    ndbg(lua, &rendered);
    ndbg(lua, &message);

    Ok(format!("hello"))
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

#[allow(dead_code)]
fn ndbg<'a, T: std::fmt::Debug>(lua: &Lua, value: &'a T) -> &'a T {
    let print = lua.globals().get::<LuaFunction>("print").unwrap();
    // let inspect = lua.globals().get::<LuaTable>("vim").unwrap();
    print.call::<()>(format!("{value:?}")).unwrap();
    value
}
