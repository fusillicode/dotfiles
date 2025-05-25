use mlua::prelude::*;

pub mod path_filter;
pub mod related_info_filter;
pub mod typos_lsp_filter;

pub trait DiagnosticsFilter {
    fn keep(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool>;
}
