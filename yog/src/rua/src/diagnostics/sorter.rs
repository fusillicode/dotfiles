use mlua::prelude::*;

/// Sort LSP diagnostics based on their severity.
/// If the severity cannot be found in the [`LuaTable`] representing the LSP diagnostic uses the
/// position of the diagnostic in the input [`LuaTable`] as key for sorting.
pub fn sort(lua: &Lua, lsp_diags: LuaTable) -> LuaResult<LuaTable> {
    let mut lsp_diags_by_severity = lsp_diags
        .sequence_values::<LuaTable>()
        .flatten()
        .enumerate()
        .map(|(idx, lsp_diag)| (lsp_diag.get::<usize>("severity").unwrap_or(idx), lsp_diag))
        .collect::<Vec<_>>();

    lsp_diags_by_severity.sort_by(|(sev_a, _), (sev_b, _)| sev_a.cmp(sev_b));

    lua.create_sequence_from(lsp_diags_by_severity.iter().map(|(_, lsp_diag)| lsp_diag))
}
