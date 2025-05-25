use mlua::prelude::*;

pub mod path_filter;
pub mod related_info_filter;
pub mod unwanted_lsp_msgs_filter;

pub trait DiagnosticsFilter {
    fn keep_diagnostic(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool>;
}
