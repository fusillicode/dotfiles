use mlua::prelude::*;

pub mod filtering;
pub mod formatting;

#[derive(Debug)]
pub struct Diagnostic {
    source: Option<String>,
    code: Option<String>,
    message: Option<String>,
    user_data: Option<UserData>,
}

#[derive(Debug)]
pub struct UserData {
    lsp: Option<Lsp>,
    message: Option<String>,
}

#[derive(Debug)]
pub struct Lsp {
    source: Option<String>,
    code: Option<String>,
    data: Option<LspData>,
}

#[derive(Debug)]
pub struct LspData {
    rendered: Option<String>,
}

impl FromLua for Diagnostic {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        if let LuaValue::Table(table) = value {
            let out = Self {
                source: table.get("source")?,
                code: table.get("code")?,
                message: table.get("message")?,
                user_data: table.get("user_data")?,
            };
            return Ok(out);
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "Diagnostic".into(),
            message: Some(format!("expected a table got {value:?}")),
        })
    }
}

impl FromLua for UserData {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::Table(table) = value {
            let out = UserData {
                message: table.get("message")?,
                lsp: table.get("lsp")?,
            };
            return Ok(out);
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "UserData".into(),
            message: Some(format!("expected a table got {value:?}")),
        })
    }
}

impl FromLua for Lsp {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::Table(table) = value {
            let out = Lsp {
                source: table.get("source")?,
                code: table.get("code")?,
                data: table.get("data")?,
            };
            return Ok(out);
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "Lsp".into(),
            message: Some(format!("expected a table got {value:?}")),
        })
    }
}

impl FromLua for LspData {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let LuaValue::Table(table) = value {
            let out = LspData {
                rendered: table.get("rendered")?,
            };
            return Ok(out);
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "Lsp".into(),
            message: Some(format!("expected a table got {value:?}")),
        })
    }
}
