use std::collections::HashMap;

use mlua::prelude::*;

use crate::utils::dig;

/// Returns the formatted [`String`] representation of the statusline.
pub fn draw(
    _lua: &Lua,
    (curr_buf_nr, curr_buf_path, diags): (LuaInteger, LuaString, LuaTable),
) -> LuaResult<String> {
    let mut statusline = Statusline {
        curr_buf_path: curr_buf_path.to_string_lossy(),
        curr_buf_diags: HashMap::new(),
        workspace_diags: HashMap::new(),
    };

    for diag in diags.sequence_values::<LuaTable>().flatten() {
        let severity = dig::<Severity>(&diag, &["severity"])?;
        if curr_buf_nr == dig::<i64>(&diag, &["bufnr"])? {
            *statusline.curr_buf_diags.entry(severity).or_insert(0) += 1;
        }
        *statusline.workspace_diags.entry(severity).or_insert(0) += 1;
    }

    Ok(statusline.draw())
}

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
enum Severity {
    Error,
    Warn,
    Info,
    Hint,
}

impl Severity {
    const VARIANTS: &'static [Self] = &[Self::Error, Self::Warn, Self::Info, Self::Hint];

    fn draw_diagnostics(&self, diags_count: i32) -> String {
        let (hg_group, sym) = match self {
            Severity::Error => ("DiagnosticStatusLineError", "E"),
            Severity::Warn => ("DiagnosticStatusLineWarn", "W"),
            Severity::Info => ("DiagnosticStatusLineInfo", "I"),
            Severity::Hint => ("DiagnosticStatusLineHint", "H"),
        };
        format!("%#{}#{}:{diags_count}", hg_group, sym)
    }
}

impl FromLua for Severity {
    fn from_lua(value: LuaValue, _: &Lua) -> LuaResult<Self> {
        match value.as_i32().ok_or_else(|| {
            mlua::Error::runtime(format!("cannot convert LuaValue {value:?} to i32"))
        })? {
            1 => Ok(Self::Error),
            2 => Ok(Self::Warn),
            3 => Ok(Self::Info),
            4 => Ok(Self::Hint),
            unexpected => Err(mlua::Error::runtime(format!(
                "unexpected i32 {unexpected} for Severity"
            ))),
        }
    }
}

#[derive(Debug)]
struct Statusline {
    curr_buf_path: String,
    curr_buf_diags: HashMap<Severity, i32>,
    workspace_diags: HashMap<Severity, i32>,
}

impl Statusline {
    fn draw(&self) -> String {
        let mut curr_buf_diags = Severity::VARIANTS
            .iter()
            .filter_map(|s| self.curr_buf_diags.get(s).map(|c| s.draw_diagnostics(*c)))
            .collect::<Vec<_>>()
            .join(" ");

        let workspace_diags = Severity::VARIANTS
            .iter()
            .filter_map(|s| self.workspace_diags.get(s).map(|c| s.draw_diagnostics(*c)))
            .collect::<Vec<_>>()
            .join(" ");

        if !curr_buf_diags.is_empty() {
            curr_buf_diags.push(' ');
        }

        format!(
            "{curr_buf_diags}%#StatusLine#{} %m %r%={workspace_diags}",
            self.curr_buf_path
        )
    }
}
