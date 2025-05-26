use mlua::prelude::*;

pub mod buffers;
pub mod msgs_blacklist;
pub mod related_info;

pub trait DiagnosticsFilter {
    fn keep_diagnostic(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool>;
}
