use mlua::prelude::*;

pub mod buffers;
pub mod lsps_msgs_blacklist;
pub mod lsps_related_info;

pub trait DiagnosticsFilter {
    fn keep_diagnostic(&self, buf_path: &str, lsp_diag: &LuaTable) -> LuaResult<bool>;
}
