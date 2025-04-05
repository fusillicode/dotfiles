use std::str::FromStr;

use fkr::FkrOption;
use mlua::prelude::*;
use strum::Display;
use strum::EnumIter;
use strum::EnumString;
use strum::IntoEnumIterator;

pub fn get_cmds(lua: &Lua, _: Option<LuaString>) -> LuaResult<LuaTable> {
    let mut fkr_cmds = vec![];
    for fkr_arg in FkrArg::iter() {
        let fkr_cmd = lua.create_table()?;
        fkr_cmd.set("name", fkr_arg.cmd_name())?;
        fkr_cmd.set("fkr_arg", fkr_arg.to_string())?;
        fkr_cmds.push(fkr_cmd);
    }
    lua.create_sequence_from(fkr_cmds)
}

pub fn gen_value(_lua: &Lua, fkr_arg: FkrArg) -> LuaResult<String> {
    Ok(FkrOption::from(fkr_arg).gen_string())
}

#[derive(Debug, PartialEq, Display, EnumString, EnumIter)]
pub enum FkrArg {
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

impl FkrArg {
    pub fn cmd_name(&self) -> &str {
        match self {
            FkrArg::Uuidv4 => "FkrUuidv4",
            FkrArg::Uuidv7 => "FkrUuidv7",
            FkrArg::Email => "FkrEmail",
            FkrArg::UserAgent => "FkrUserAgent",
            FkrArg::IPv4 => "FkrIPv4",
            FkrArg::IPv6 => "FkrIPv6",
            FkrArg::MACAddress => "FkrMACAddress",
        }
    }
}

impl FromLua for FkrArg {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        if let LuaValue::String(ref lua_string) = value {
            return Self::from_str(lua_string.to_string_lossy().as_str()).map_err(|e| {
                mlua::Error::FromLuaConversionError {
                    from: value.type_name(),
                    to: "FkrArg".into(),
                    message: Some(format!("error parsing string {lua_string:?}, error: {e:?}")),
                }
            });
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "FkrArg".into(),
            message: Some(format!("expected a string got {value:?}")),
        })
    }
}

impl From<FkrArg> for FkrOption {
    fn from(value: FkrArg) -> Self {
        match value {
            FkrArg::Uuidv4 => FkrOption::Uuidv4,
            FkrArg::Uuidv7 => FkrOption::Uuidv7,
            FkrArg::Email => FkrOption::Email,
            FkrArg::UserAgent => FkrOption::UserAgent,
            FkrArg::IPv4 => FkrOption::IPv4,
            FkrArg::IPv6 => FkrOption::IPv6,
            FkrArg::MACAddress => FkrOption::MACAddress,
        }
    }
}

impl From<FkrOption> for FkrArg {
    fn from(value: FkrOption) -> Self {
        match value {
            FkrOption::Uuidv4 => FkrArg::Uuidv4,
            FkrOption::Uuidv7 => FkrArg::Uuidv7,
            FkrOption::Email => FkrArg::Email,
            FkrOption::UserAgent => FkrArg::UserAgent,
            FkrOption::IPv4 => FkrArg::IPv4,
            FkrOption::IPv6 => FkrArg::IPv6,
            FkrOption::MACAddress => FkrArg::MACAddress,
        }
    }
}
