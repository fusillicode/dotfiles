use mlua::prelude::*;

pub fn draw(_lua: &Lua, (cur_lnum, signs): (LuaString, Signs)) -> LuaResult<String> {
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
        let mut out: String = [
            &self.error,
            &self.warn,
            &self.info,
            &self.hint,
            &self.ok,
            &self.git,
        ]
        .iter()
        .filter_map(|s| s.as_ref().map(Sign::draw))
        .collect();

        out.insert_str(0, " ");
        out.push_str(" %=% ");
        out.push_str(&self.cur_lnum);
        out.push_str(" ");
        out
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
