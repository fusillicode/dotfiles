use mlua::prelude::*;

use crate::diagnostics::filters::buffers::skip_diagnostics_for_buf_path;
use crate::diagnostics::filters::related_info::RelatedInfoFilter;

/// Filters out the LSP diagnostics based on the coded filters.
pub fn filter_diagnostics(
    lua: &Lua,
    (buf_path, lsp_diags): (LuaString, LuaTable),
) -> LuaResult<LuaTable> {
    let buf_path = buf_path.to_string_lossy();
    if skip_diagnostics_for_buf_path(&buf_path) {
        return lua.create_sequence_from::<LuaTable>(vec![]);
    }

    // Order of filters is IMPORTANT.
    // The first filter that returns false skips the LSP diagnostic.
    let mut filters = crate::diagnostics::filters::msg_blacklist::configured_filters();
    filters.push(Box::new(RelatedInfoFilter::new(&lsp_diags)?));

    let mut out = vec![];
    // Using [`.pairs`] and [`LuaValue`] to get a & to the LSP diagnostic [`LuaTable`] and avoid
    // cloning it when calling the [`DiagnosticsFilter::apply`].
    for (_, lua_value) in lsp_diags.pairs::<usize, LuaValue>().flatten() {
        let lsp_diag = lua_value.as_table().ok_or_else(|| {
            mlua::Error::RuntimeError(format!("cannot get LuaTable from LuaValue {lua_value:?}"))
        })?;

        let mut skip_diagnostic = false;
        for filter in &filters {
            if filter.skip_diagnostic(&buf_path, lsp_diag)? {
                skip_diagnostic = true;
                break;
            }
        }
        if skip_diagnostic {
            continue;
        }
        out.push(lsp_diag.clone());
    }

    lua.create_sequence_from(out)
}
