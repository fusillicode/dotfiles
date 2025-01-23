use mlua::prelude::*;

/// Returns the formatted [`String`] representation of the statuscolumn.
pub fn draw(
    _lua: &Lua,
    (curbuf_type, cur_lnum, signs): (LuaString, LuaString, Signs),
) -> LuaResult<String> {
    match curbuf_type.to_string_lossy().as_str() {
        "grug-far" => Ok(" ".into()),
        _ => {
            let mut statuscolumn = Statuscolumn::new(cur_lnum.to_string_lossy());

            for sign in signs.0 {
                match sign.sign_hl_group.as_str() {
                    "DiagnosticSignError" => statuscolumn.error = Some(sign),
                    "DiagnosticSignWarn" => statuscolumn.warn = Some(sign),
                    "DiagnosticSignInfo" => statuscolumn.info = Some(sign),
                    "DiagnosticSignHint" => statuscolumn.hint = Some(sign),
                    "DiagnosticSignOk" => statuscolumn.ok = Some(sign),
                    git if git.contains("GitSigns") => statuscolumn.git = Some(sign),
                    _ => (),
                };
            }

            Ok(statuscolumn.draw())
        }
    }
}

pub struct Signs(Vec<Sign>);

impl FromLua for Signs {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::Table(table) = value {
            let signs = table
                .sequence_values::<Sign>()
                .collect::<Result<Vec<Sign>, _>>()?;

            return Ok(Signs(signs));
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "Signs".into(),
            message: Some(format!("expected a table got {value:?}")),
        })
    }
}

pub struct Sign {
    sign_hl_group: String,
    // Option due to grug-far buffers
    sign_text: Option<String>,
}

impl FromLua for Sign {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::Table(table) = value {
            let sign = table.get::<LuaTable>(4)?;
            return Ok(Sign {
                sign_hl_group: sign.get("sign_hl_group")?,
                sign_text: sign.get("sign_text")?,
            });
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "Sign".into(),
            message: Some(format!("expected a table got {value:?}")),
        })
    }
}

impl Sign {
    fn draw(&self) -> String {
        format!(
            "%#{}#{}%*",
            self.sign_hl_group,
            self.sign_text.as_ref().map(|x| x.trim()).unwrap_or("")
        )
    }
}

#[derive(Default)]
struct Statuscolumn {
    error: Option<Sign>,
    warn: Option<Sign>,
    info: Option<Sign>,
    hint: Option<Sign>,
    ok: Option<Sign>,
    git: Option<Sign>,
    cur_lnum: String,
}

impl Statuscolumn {
    fn new(cur_lnum: String) -> Self {
        Self {
            cur_lnum,
            ..Default::default()
        }
    }
}

impl Statuscolumn {
    fn draw(&self) -> String {
        let mut out = String::new();

        let diag_sign = [&self.error, &self.warn, &self.info, &self.hint, &self.ok]
            .iter()
            .find_map(|s| s.as_ref().map(Sign::draw))
            .unwrap_or_else(|| " ".into());

        out.push_str(&diag_sign);
        out.push_str(
            &self
                .git
                .as_ref()
                .map(Sign::draw)
                .unwrap_or_else(|| " ".to_string()),
        );
        out.push_str(" %=% ");
        out.push_str(&self.cur_lnum);
        out.push(' ');

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statuscolumn_draw_works_as_expected() {
        let statuscolumn = Statuscolumn {
            cur_lnum: "42".into(),
            ..Default::default()
        };
        assert_eq!("   %=% 42 ", &statuscolumn.draw());

        let statuscolumn = Statuscolumn {
            error: Some(Sign {
                sign_hl_group: "foo".into(),
                sign_text: Some("E".into()),
            }),
            cur_lnum: "42".into(),
            ..Default::default()
        };
        assert_eq!("%#foo#E%*  %=% 42 ", &statuscolumn.draw());

        let statuscolumn = Statuscolumn {
            error: Some(Sign {
                sign_hl_group: "err".into(),
                sign_text: Some("E".into()),
            }),
            warn: Some(Sign {
                sign_hl_group: "warn".into(),
                sign_text: Some("W".into()),
            }),
            cur_lnum: "42".into(),
            ..Default::default()
        };
        assert_eq!("%#err#E%*  %=% 42 ", &statuscolumn.draw());

        let statuscolumn = Statuscolumn {
            git: Some(Sign {
                sign_hl_group: "git".into(),
                sign_text: Some("|".into()),
            }),
            cur_lnum: "42".into(),
            ..Default::default()
        };
        assert_eq!(" %#git#|%* %=% 42 ", &statuscolumn.draw());

        let statuscolumn = Statuscolumn {
            error: Some(Sign {
                sign_hl_group: "err".into(),
                sign_text: Some("E".into()),
            }),
            warn: Some(Sign {
                sign_hl_group: "warn".into(),
                sign_text: Some("W".into()),
            }),
            git: Some(Sign {
                sign_hl_group: "git".into(),
                sign_text: Some("|".into()),
            }),
            cur_lnum: "42".into(),
            ..Default::default()
        };
        assert_eq!("%#err#E%*%#git#|%* %=% 42 ", &statuscolumn.draw());
    }
}
