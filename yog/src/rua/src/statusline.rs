use std::collections::HashMap;

use mlua::prelude::*;

use crate::utils::dig;

/// Returns the formatted [`String`] representation of the statusline.
pub fn draw(
    _lua: &Lua,
    (cur_buf_nr, cur_buf_path, diags): (LuaNumber, LuaString, LuaTable),
) -> LuaResult<String> {
    let mut statusline = Statusline {
        cuf_buf_path: cur_buf_path.to_string_lossy(),
        cur_buf_diags: HashMap::new(),
        workspace_diags: HashMap::new(),
    };

    for diag in diags.sequence_values::<LuaTable>().flatten() {
        let severity = dig::<u8>(&diag, &["severity"])?;
        if cur_buf_nr == dig::<f64>(&diag, &["bufnr"])? {
            *statusline.cur_buf_diags.entry(severity).or_insert(0) += 1;
        }
        *statusline.workspace_diags.entry(severity).or_insert(0) += 1;
    }

    //    (buffer_errors ~= 0 and '%#DiagnosticStatusLineError#' .. 'E:' .. buffer_errors .. ' ' or '')
    // .. (buffer_warns ~= 0 and '%#DiagnosticStatusLineWarn#' .. 'W:' .. buffer_warns .. ' ' or '')
    // .. (buffer_infos ~= 0 and '%#DiagnosticStatusLineInfo#' .. 'I:' .. buffer_infos .. ' ' or '')
    // .. (buffer_hints ~= 0 and '%#DiagnosticStatusLineHint#' .. 'H:' .. buffer_hints .. ' ' or '')
    // .. '%#StatusLine#'
    // -- https://stackoverflow.com/a/45244610
    // .. current_buffer_path() .. ' %m %r'
    // .. '%='
    // .. (workspace_errors ~= 0 and '%#DiagnosticStatusLineError#' .. 'E:' .. workspace_errors .. ' ' or '')
    // .. (workspace_warns ~= 0 and '%#DiagnosticStatusLineWarn#' .. 'W:' .. workspace_warns .. ' ' or '')
    // .. (workspace_infos ~= 0 and '%#DiagnosticStatusLineInfo#' .. 'I:' .. workspace_infos .. ' ' or '')
    // .. (workspace_hints ~= 0 and '%#DiagnosticStatusLineHint#' .. 'H:' .. workspace_hints .. ' ' or '')

    Ok(format!("▶ {cur_buf_nr:?} {cur_buf_path:?} {statusline:?}]"))
}

#[derive(Debug)]
struct Statusline {
    cuf_buf_path: String,
    cur_buf_diags: HashMap<u8, i32>,
    workspace_diags: HashMap<u8, i32>,
}

impl Statusline {
    fn format(&self) -> String {
        todo!()
    }
}
