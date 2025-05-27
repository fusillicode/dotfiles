use mlua::prelude::*;
use mlua::Error;

use crate::diagnostics::filters::buffer::BufferFilter;
use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::DiagnosticsFilters;

/// Filters out the LSP diagnostics based on the coded filters.
pub fn filter_diagnostics(
    lua: &Lua,
    (buf_path, lsp_diags): (LuaString, LuaTable),
) -> LuaResult<LuaTable> {
    let buf_path = buf_path.to_string_lossy();
    // Keeping this as a separate filter because it kind short circuits the whole filtering and
    // doesn't require any LSP diagnostics to apply its logic.
    if BufferFilter::new().skip_diagnostic(&buf_path, None)? {
        return lua.create_sequence_from::<LuaTable>(vec![]);
    };

    let filters = DiagnosticsFilters::all(&lsp_diags)?;

    let mut out = vec![];
    // Using [`.pairs`] and [`LuaValue`] to get a & to the LSP diagnostic [`LuaTable`] and avoid
    // cloning it when passing it to the filter.
    for (_, lua_value) in lsp_diags.pairs::<usize, LuaValue>().flatten() {
        let lsp_diag = lua_value.as_table().ok_or_else(|| {
            Error::RuntimeError(format!("cannot get LuaTable from LuaValue {lua_value:?}"))
        })?;
        if filters.skip_diagnostic(&buf_path, Some(lsp_diag))? {
            continue;
        }
        out.push(lsp_diag.clone());
    }

    lua.create_sequence_from(out)
}
