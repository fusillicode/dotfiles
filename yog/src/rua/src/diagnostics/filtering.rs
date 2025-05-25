use mlua::prelude::*;

use crate::diagnostics::filters::path_filter::no_diagnostics_for_path;
use crate::diagnostics::filters::related_info_filter::RelatedInfoFilter;
use crate::diagnostics::filters::DiagnosticsFilter;

/// Filters out the LSP diagnostics based on the coded filters.
pub fn filter_diagnostics(
    lua: &Lua,
    (buf_path, lsp_diags): (LuaString, LuaTable),
) -> LuaResult<LuaTable> {
    let buf_path = buf_path.to_string_lossy();
    if let Some(out) = no_diagnostics_for_path(lua, &buf_path) {
        return out;
    }

    let related_info_filter = RelatedInfoFilter::new(&lsp_diags)?;

    let mut out = vec![];
    for lsp_diag in lsp_diags.sequence_values::<LuaTable>().flatten() {
        related_info_filter.apply(&mut out, &buf_path, lsp_diag)?;
    }

    lua.create_sequence_from(out)
}
