use mlua::prelude::*;

pub fn draw(_lua: &Lua, signs: Signs) -> LuaResult<String> {
    // local git_sign, error_sign, warn_sign, hint_sign, info_sign, ok_sign
    // for _, sign in ipairs(line_signs) do
    //   local sign_details = sign[4]
    //
    //   if sign_details.sign_hl_group:sub(1, 8) == 'GitSigns' then
    //     git_sign = sign_details
    //   elseif sign_details.sign_hl_group == 'DiagnosticSignError' then
    //     error_sign = sign_details
    //   elseif sign_details.sign_hl_group == 'DiagnosticSignWarn' then
    //     warn_sign = sign_details
    //   elseif sign_details.sign_hl_group == 'DiagnosticSignInfo' then
    //     info_sign = sign_details
    //   elseif sign_details.sign_hl_group == 'DiagnosticSignHint' then
    //     hint_sign = sign_details
    //   elseif sign_details.sign_hl_group == 'DiagnosticSignOk' then
    //     ok_sign = sign_details
    //   end
    // end
    //
    // return format_extmark(error_sign or warn_sign or info_sign or hint_sign or ok_sign)
    //     .. format_extmark(git_sign)
    //     .. ' %=%{v:lnum} '
    Ok("foo".into())
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
    ns_id: i64,
    priority: usize,
    right_gravity: bool,
    sign_hl_group: String,
    sign_text: String,
}

impl FromLua for Sign {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::Table(table) = value {
            let sign = table.get::<LuaTable>(4)?;
            return Ok(Sign {
                ns_id: sign.get("ns_id")?,
                priority: sign.get("priority")?,
                right_gravity: sign.get("right_gravity")?,
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
