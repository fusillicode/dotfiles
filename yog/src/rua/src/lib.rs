use mlua::prelude::*;

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
fn format_diagnostic(_: &Lua, lsp_diagnostic: LuaTable) -> LuaResult<String> {
    let oo = dig::<String>(lsp_diagnostic, &["user_data", "lsp", "data", "rendered"]);
    Ok(format!("hello {oo:?}"))
}

fn dig<T: FromLua>(tbl: LuaTable, keys: &[&str]) -> LuaResult<T> {
    match keys {
        [] => Err(mlua::Error::RuntimeError("no keys supplied".into())),
        [leaf] => tbl.raw_get::<T>(*leaf),
        [path @ .., leaf] => {
            let mut res: Option<LuaTable> = None;
            for key in path {
                res = Some(tbl.raw_get::<LuaTable>(*key)?);
            }
            if let Some(res) = res {
                return res.raw_get::<T>(*leaf);
            }
            Err(mlua::Error::RuntimeError(format!(
                "didn't find {keys:?} in {tbl:?}"
            )))
        }
    }
}

#[mlua::lua_module]
fn rua(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("format_diagnostic", lua.create_function(format_diagnostic)?)?;
    Ok(exports)
}

// #[cfg(test)]
// mod tests {
//     use mlua::Lua;
//
//     use super::*;
//
//     #[test]
//     fn test_dig_returns_an_error_in_case_of_no_keys_supplied() {
//         let lua = Lua::new();
//         let tbl: LuaTable = lua.create_table();
//         let res = dig::<usize>(tbl, &["foo", "bar"]);
//         dbg!(res);
//         panic!()
//     }
//
//     #[test]
//     fn test_dig_returns_an_error_in_case_of_empty_table() {
//         let lua = Lua::new();
//         let tbl: LuaTable = lua.load(r#"{}"#).eval().unwrap();
//         let res = dig::<usize>(tbl, &["foo", "bar"]);
//         dbg!(res);
//         panic!()
//     }
//
//     #[test]
//     fn test_dig_returns_the_value_under_the_supplied_keys() {
//         let lua = Lua::new();
//         let tbl: LuaTable = lua
//             .load(
//                 r#"
//                     {
//                         "foo" = {
//                             "bar" = 42
//                         },
//                         "baz" = "boo"
//                     }
//                 "#,
//             )
//             .eval()
//             .unwrap();
//         let res = dig::<usize>(tbl, &["foo", "bar"]);
//         dbg!(res);
//         panic!()
//     }
// }
