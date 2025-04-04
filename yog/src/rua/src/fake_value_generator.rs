use std::str::FromStr;

use fkr::strum_macros::Display;
use fkr::strum_macros::EnumString;
use fkr::FkrOption;
use mlua::prelude::*;

pub fn gen_cmds(lua: &Lua, _: Option<LuaString>) -> LuaResult<LuaTable> {
    let cmds = lua.create_table()?;
    for fkr_opt in FkrOption::to_vec() {
        cmds.set("title", get_title(&fkr_opt))?;
        cmds.set("cmd", get_lua_gen_value_cmd(&fkr_opt))?;
    }
    Ok(cmds)
}

pub fn gen_value(_lua: &Lua, lua_fkr_opt: LuaFkrOption) -> LuaResult<String> {
    Ok(FkrOption::from(lua_fkr_opt).gen_string())
}

#[derive(Debug, PartialEq, Display, EnumString)]
pub enum LuaFkrOption {
    #[strum(to_string = "uuid-v4")]
    Uuidv4,
    #[strum(to_string = "uuid-v7")]
    Uuidv7,
    #[strum(to_string = "email")]
    Email,
    #[strum(to_string = "user-agent")]
    UserAgent,
    #[strum(to_string = "ipv4")]
    IPv4,
    #[strum(to_string = "ipv6")]
    IPv6,
    #[strum(to_string = "mac-addr")]
    MACAddress,
}

impl FromLua for LuaFkrOption {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        if let LuaValue::String(ref lua_string) = value {
            return Self::from_str(lua_string.to_string_lossy().as_str()).map_err(|e| {
                mlua::Error::FromLuaConversionError {
                    from: value.type_name(),
                    to: "LuaFkrOption".into(),
                    message: Some(format!("error parsing string {lua_string:?}, error: {e:?}")),
                }
            });
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "LuaFkrOption".into(),
            message: Some(format!("expected a string got {value:?}")),
        })
    }
}

impl From<LuaFkrOption> for FkrOption {
    fn from(value: LuaFkrOption) -> Self {
        match value {
            LuaFkrOption::Uuidv4 => FkrOption::Uuidv4,
            LuaFkrOption::Uuidv7 => FkrOption::Uuidv7,
            LuaFkrOption::Email => FkrOption::Email,
            LuaFkrOption::UserAgent => FkrOption::UserAgent,
            LuaFkrOption::IPv4 => FkrOption::IPv4,
            LuaFkrOption::IPv6 => FkrOption::IPv6,
            LuaFkrOption::MACAddress => FkrOption::MACAddress,
        }
    }
}

impl From<FkrOption> for LuaFkrOption {
    fn from(value: FkrOption) -> Self {
        match value {
            FkrOption::Uuidv4 => LuaFkrOption::Uuidv4,
            FkrOption::Uuidv7 => LuaFkrOption::Uuidv7,
            FkrOption::Email => LuaFkrOption::Email,
            FkrOption::UserAgent => LuaFkrOption::UserAgent,
            FkrOption::IPv4 => LuaFkrOption::IPv4,
            FkrOption::IPv6 => LuaFkrOption::IPv6,
            FkrOption::MACAddress => LuaFkrOption::MACAddress,
        }
    }
}

fn get_title(fkr_opt: &FkrOption) -> &str {
    match fkr_opt {
        FkrOption::Uuidv4 => "Fake Uuidv4",
        FkrOption::Uuidv7 => "Fake Uuidv7",
        FkrOption::Email => "Fake Email",
        FkrOption::UserAgent => "Fake UserAgent",
        FkrOption::IPv4 => "Fake IPv4",
        FkrOption::IPv6 => "Fake IPv6",
        FkrOption::MACAddress => "Fake MACAddress",
    }
}

fn get_lua_gen_value_cmd(fkr_opt: &FkrOption) -> String {
    format!(
        ":lua require('rua').gen_value('{}')",
        LuaFkrOption::from(*fkr_opt)
    )
}
