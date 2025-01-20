use mlua::prelude::*;

pub fn draw(_lua: &Lua, signs: Signs) -> LuaResult<String> {
    let mut statuscolumn = Statuscolumn::default();

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

#[derive(Default)]
struct Statuscolumn {
    error: Option<Sign>,
    warn: Option<Sign>,
    info: Option<Sign>,
    hint: Option<Sign>,
    ok: Option<Sign>,
    git: Option<Sign>,
}

impl Statuscolumn {
    fn draw(&self) -> String {
        [
            &self.error,
            &self.warn,
            &self.info,
            &self.hint,
            &self.ok,
            &self.git,
        ]
        .iter()
        .filter_map(|s| s.as_ref().map(Sign::draw))
        .collect()
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
    sign_text: String,
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
        format!("%#{}#{}%*", self.sign_hl_group, self.sign_text.trim())
    }
}
