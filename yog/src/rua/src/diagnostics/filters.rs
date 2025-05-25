use mlua::prelude::*;

pub mod path_filter;
pub mod related_info_filter;
pub mod typos_lsp_filter;

pub trait DiagnosticsFilter {
    fn apply(&self, out: &mut Vec<LuaTable>, buf_path: &str, lsp_diag: LuaTable) -> LuaResult<()>;
}
