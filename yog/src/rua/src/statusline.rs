use std::collections::HashMap;

use mlua::prelude::*;

/// Returns the formatted [`String`] representation of the statusline.
pub fn draw(
    _lua: &Lua,
    (cur_buf_nr, cur_buf_path, diags): (LuaInteger, LuaString, Diagnostics),
) -> LuaResult<String> {
    let mut statusline = Statusline {
        cur_buf_path: cur_buf_path.to_string_lossy(),
        cur_buf_diags: HashMap::new(),
        workspace_diags: HashMap::new(),
    };

    for diag in diags.0 {
        if cur_buf_nr == diag.bufnr {
            *statusline.cur_buf_diags.entry(diag.severity).or_insert(0) += 1;
        }
        *statusline.workspace_diags.entry(diag.severity).or_insert(0) += 1;
    }

    Ok(statusline.draw())
}

pub struct Diagnostics(Vec<Diagnostic>);

impl FromLua for Diagnostics {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::Table(table) = value {
            let diagnostics = table
                .sequence_values::<Diagnostic>()
                .collect::<Result<Vec<Diagnostic>, _>>()?;

            return Ok(Diagnostics(diagnostics));
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "Diagnostics".into(),
            message: Some(format!("expected a table got {value:#?}")),
        })
    }
}

pub struct Diagnostic {
    bufnr: i64,
    severity: Severity,
}

impl FromLua for Diagnostic {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::Table(table) = value {
            return Ok(Diagnostic {
                bufnr: table.get("bufnr")?,
                severity: table.get("severity")?,
            });
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "Diagnostic".into(),
            message: Some(format!("expected a table got {value:#?}")),
        })
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub enum Severity {
    Error,
    Warn,
    Info,
    Hint,
}

impl FromLua for Severity {
    fn from_lua(value: LuaValue, _: &Lua) -> LuaResult<Self> {
        if let LuaValue::Integer(int) = value {
            return match int {
                1 => Ok(Self::Error),
                2 => Ok(Self::Warn),
                3 => Ok(Self::Info),
                4 => Ok(Self::Hint),
                _ => Err(mlua::Error::FromLuaConversionError {
                    from: value.type_name(),
                    to: "Severity".into(),
                    message: Some(format!("unexpected int {int}")),
                }),
            };
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "Severity".into(),
            message: Some(format!("expected an integer got {value:#?}")),
        })
    }
}

impl Severity {
    const ORDER: &'static [Self] = &[Self::Error, Self::Warn, Self::Info, Self::Hint];

    fn draw_diagnostics(&self, diags_count: i32) -> String {
        if diags_count == 0 {
            return "".into();
        }
        let (hg_group, sym) = match self {
            Severity::Error => ("DiagnosticStatusLineError", "E"),
            Severity::Warn => ("DiagnosticStatusLineWarn", "W"),
            Severity::Info => ("DiagnosticStatusLineInfo", "I"),
            Severity::Hint => ("DiagnosticStatusLineHint", "H"),
        };
        format!("%#{hg_group}#{sym}:{diags_count}")
    }
}

#[derive(Debug)]
struct Statusline {
    cur_buf_path: String,
    cur_buf_diags: HashMap<Severity, i32>,
    workspace_diags: HashMap<Severity, i32>,
}

impl Statusline {
    fn draw(&self) -> String {
        let mut cur_buf_diags = Severity::ORDER
            .iter()
            .filter_map(|s| self.cur_buf_diags.get(s).map(|c| s.draw_diagnostics(*c)))
            .collect::<Vec<_>>()
            .join(" ");

        let workspace_diags = Severity::ORDER
            .iter()
            .filter_map(|s| self.workspace_diags.get(s).map(|c| s.draw_diagnostics(*c)))
            .collect::<Vec<_>>()
            .join(" ");

        if !cur_buf_diags.is_empty() {
            cur_buf_diags.push(' ');
        }

        format!(
            "{cur_buf_diags}%#StatusLine#{} %m %r%={workspace_diags}",
            self.cur_buf_path
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_line_draw_works_as_expected() {
        for statusline in [
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: HashMap::new(),
                workspace_diags: HashMap::new(),
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: [(Severity::Info, 0)].into_iter().collect(),
                workspace_diags: HashMap::new(),
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: HashMap::new(),
                workspace_diags: [(Severity::Info, 0)].into_iter().collect(),
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: [(Severity::Info, 0)].into_iter().collect(),
                workspace_diags: [(Severity::Info, 0)].into_iter().collect(),
            },
        ] {
            let res = statusline.draw();
            assert_eq!(
                "%#StatusLine#foo %m %r%=", &res,
                "unexpected not empty diagnosticts drawn, res {res}, statusline {statusline:#?}"
            );
        }

        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(Severity::Info, 1), (Severity::Error, 3)]
                .into_iter()
                .collect(),
            workspace_diags: [(Severity::Info, 0)].into_iter().collect(),
        };
        assert_eq!(
            "%#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1 %#StatusLine#foo %m %r%=",
            &statusline.draw()
        );

        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(Severity::Info, 0)].into_iter().collect(),
            workspace_diags: [(Severity::Info, 1), (Severity::Error, 3)]
                .into_iter()
                .collect(),
        };
        assert_eq!(
            "%#StatusLine#foo %m %r%=%#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1",
            &statusline.draw()
        );

        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(Severity::Hint, 3), (Severity::Warn, 2)]
                .into_iter()
                .collect(),
            workspace_diags: [(Severity::Info, 1), (Severity::Error, 3)]
                .into_iter()
                .collect(),
        };
        assert_eq!(
            "%#DiagnosticStatusLineWarn#W:2 %#DiagnosticStatusLineHint#H:3 %#StatusLine#foo %m %r%=%#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1",
            &statusline.draw()
        );
    }
}
