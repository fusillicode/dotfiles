use mlua::prelude::*;

/// Returns the formatted [`String`] representation of the statuscolumn.
pub fn draw(
    _lua: &Lua,
    (cur_buf_type, cur_lnum, signs): (LuaString, LuaString, Signs),
) -> LuaResult<String> {
    Ok(Statuscolumn::draw(
        cur_buf_type.to_string_lossy().as_str(),
        cur_lnum.to_string_lossy(),
        signs.0,
    ))
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
    fn draw(cur_buf_type: &str, cur_lnum: String, signs: Vec<Sign>) -> String {
        match cur_buf_type {
            "grug-far" => " ".into(),
            _ => Self::new(cur_lnum, signs).to_string(),
        }
    }

    fn new(cur_lnum: String, signs: Vec<Sign>) -> Self {
        let mut statuscolumn = Self {
            cur_lnum,
            ..Default::default()
        };

        for sign in signs {
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

        statuscolumn
    }
}

impl std::fmt::Display for Statuscolumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let diag_sign = [&self.error, &self.warn, &self.info, &self.hint, &self.ok]
            .iter()
            .find_map(|s| s.as_ref().map(Sign::draw))
            .unwrap_or_else(|| " ".into());

        let git_sign = self
            .git
            .as_ref()
            .map(Sign::draw)
            .unwrap_or_else(|| " ".into());

        write!(f, "{}{}%=% {} ", diag_sign, git_sign, self.cur_lnum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statuscolumn_draw_works_as_expected() {
        // No signs
        let out = Statuscolumn::draw("foo", "42".into(), vec![]);
        assert_eq!("  %=% 42 ", &out);

        // 1 diagnostic sign
        let out = Statuscolumn::draw(
            "foo",
            "42".into(),
            vec![Sign {
                sign_hl_group: "DiagnosticSignError".into(),
                sign_text: Some("E".into()),
            }],
        );
        assert_eq!("%#DiagnosticSignError#E%* %=% 42 ", &out);

        // Multiple diagnostics signs and only the higher severity sign is displayed
        let out = Statuscolumn::draw(
            "foo",
            "42".into(),
            vec![
                Sign {
                    sign_hl_group: "DiagnosticSignError".into(),
                    sign_text: Some("E".into()),
                },
                Sign {
                    sign_hl_group: "DiagnosticSignWarn".into(),
                    sign_text: Some("W".into()),
                },
            ],
        );
        assert_eq!("%#DiagnosticSignError#E%* %=% 42 ", &out);

        // git sign
        let out = Statuscolumn::draw(
            "foo",
            "42".into(),
            vec![Sign {
                sign_hl_group: "GitSignsFoo".into(),
                sign_text: Some("|".into()),
            }],
        );
        assert_eq!(" %#GitSignsFoo#|%*%=% 42 ", &out);

        // Multiple diagnostics signs and a git sign
        let out = Statuscolumn::draw(
            "foo",
            "42".into(),
            vec![
                Sign {
                    sign_hl_group: "DiagnosticSignError".into(),
                    sign_text: Some("E".into()),
                },
                Sign {
                    sign_hl_group: "DiagnosticSignWarn".into(),
                    sign_text: Some("W".into()),
                },
                Sign {
                    sign_hl_group: "GitSignsFoo".into(),
                    sign_text: Some("|".into()),
                },
            ],
        );
        assert_eq!("%#DiagnosticSignError#E%*%#GitSignsFoo#|%*%=% 42 ", &out);
    }
}
