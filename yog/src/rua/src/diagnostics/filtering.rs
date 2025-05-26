use mlua::prelude::*;

use crate::diagnostics::filters::buffer::BufferFilter;
use crate::diagnostics::filters::related_info::RelatedInfoFilter;
use crate::diagnostics::filters::DiagnosticsFilter;
use crate::diagnostics::filters::DiagnosticsFilters;

/// Filters out the LSP diagnostics based on the coded filters.
pub fn filter_diagnostics(
    lua: &Lua,
    (buf_path, lsp_diags): (LuaString, LuaTable),
) -> LuaResult<LuaTable> {
    let buf_path = buf_path.to_string_lossy();
    if BufferFilter::new().skip_diagnostic(&buf_path, None)? {
        return lua.create_sequence_from::<LuaTable>(vec![]);
    };

    // Order of filters is IMPORTANT.
    // The first filter that returns false skips the LSP diagnostic.
    let filters = {
        let mut tmp = crate::diagnostics::filters::msg_blacklist::filters();
        tmp.push(Box::new(RelatedInfoFilter::new(&lsp_diags)?));
        DiagnosticsFilters::new(tmp)
    };

    let mut out = vec![];
    // Using [`.pairs`] and [`LuaValue`] to get a & to the LSP diagnostic [`LuaTable`] and avoid
    // cloning it when calling the [`DiagnosticsFilter::apply`].
    for (_, lua_value) in lsp_diags.pairs::<usize, LuaValue>().flatten() {
        let lsp_diag = lua_value.as_table().ok_or_else(|| {
            mlua::Error::RuntimeError(format!("cannot get LuaTable from LuaValue {lua_value:?}"))
        })?;
        if filters.skip_diagnostic(&buf_path, lsp_diag)? {
            continue;
        }
        out.push(lsp_diag.clone());
    }

    lua.create_sequence_from(out)
}
