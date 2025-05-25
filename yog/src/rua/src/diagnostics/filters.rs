use mlua::prelude::*;

pub mod path_filter;
pub mod related_info_filter;

pub trait DiagnosticsFilter {
    fn apply(&self, out: &mut Vec<LuaTable>, lsp_diag: LuaTable) -> LuaResult<()>;
}
