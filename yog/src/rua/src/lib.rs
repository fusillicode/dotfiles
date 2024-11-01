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
    let oo = dig::<String>(lua, &lsp_diagnostic, &["user_data", "lsp", "data"]);
    Ok(format!("hello {oo:?}"))
}

fn dig<T: FromLua>(lua: &Lua, tbl: &LuaTable, keys: &[&str]) -> LuaResult<T> {
    match keys {
        [] => Err(mlua::Error::RuntimeError("no keys supplied".into())),
        [leaf] => tbl.raw_get::<T>(*leaf),
        [path @ .., leaf] => {
            ndbg(lua, &leaf);
            let mut res: LuaTable = tbl.clone();
            for key in path {
                ndbg(lua, &key);
                res = res.raw_get::<LuaTable>(*key)?;
            }
            res.raw_get::<T>(*leaf)
        }
    }
}

fn ndbg<'a, T: std::fmt::Debug>(lua: &Lua, value: &'a T) -> &'a T {
    let print = lua.globals().get::<LuaFunction>("print").unwrap();
    // let inspect = lua.globals().get::<LuaTable>("vim").unwrap();
    print.call::<()>(format!("{value:?}")).unwrap();
    value
}
