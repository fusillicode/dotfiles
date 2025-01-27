use mlua::prelude::*;

/// Returns the formatted [`String`] representation of an LSP diagnostic.
pub fn format_diagnostic(_lua: &Lua, diag: Diagnostic) -> LuaResult<String> {
    let msg = get_msg(&diag).map_or_else(
        || format!("no message in {diag:?}"),
        |s| s.trim_end_matches('.').to_string(),
    );
    let src = get_src(&diag).map_or_else(|| format!("no source in {diag:?}"), str::to_string);
    let code = get_code(&diag);
    let src_and_code = code.map_or_else(|| src.clone(), |c| format!("{src}: {c}"));

    Ok(format!("▶ {msg} [{src_and_code}]"))
}

/// Extracts LSP diagnostic message from [`LspData::rendered`] or directly from the supplied [`Diagnostic`].
fn get_msg(diag: &Diagnostic) -> Option<&str> {
    diag.user_data
        .as_ref()
        .and_then(|user_data| {
            user_data
                .lsp
                .as_ref()
                .and_then(|lsp| {
                    lsp.data
                        .as_ref()
                        .and_then(|lsp_data| lsp_data.rendered.as_deref())
                        .or(lsp.message.as_deref())
                })
                .or(diag.message.as_deref())
        })
        .or(diag.message.as_deref())
}

/// Extracts the "source" from [`Diagnostic::user_data`] or [`Diagnostic::source`].
fn get_src(diag: &Diagnostic) -> Option<&str> {
    diag.user_data
        .as_ref()
        .and_then(|user_data| user_data.lsp.as_ref().and_then(|lsp| lsp.source.as_deref()))
        .or(diag.source.as_deref())
}

/// Extracts the "code" from [`Diagnostic::user_data`] or [`Diagnostic::code`].
fn get_code(diag: &Diagnostic) -> Option<&str> {
    diag.user_data
        .as_ref()
        .and_then(|user_data| user_data.lsp.as_ref().and_then(|lsp| lsp.code.as_deref()))
        .or(diag.code.as_deref())
}

#[derive(Debug)]
pub struct Diagnostic {
    code: Option<String>,
    message: Option<String>,
    source: Option<String>,
    user_data: Option<UserData>,
}

#[derive(Debug)]
pub struct UserData {
    lsp: Option<Lsp>,
}

#[derive(Debug)]
pub struct Lsp {
    code: Option<String>,
    data: Option<LspData>,
    message: Option<String>,
    source: Option<String>,
}

#[derive(Debug)]
pub struct LspData {
    rendered: Option<String>,
}

impl FromLua for Diagnostic {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        if let LuaValue::Table(table) = value {
            let out = Self {
                code: get_optional_value(&table, "code")?,
                message: get_optional_value(&table, "message")?,
                source: get_optional_value(&table, "source")?,
                user_data: get_optional_value(&table, "user_data")?,
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
                lsp: get_optional_value(&table, "lsp")?,
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
                code: get_optional_value(&table, "code")?,
                data: get_optional_value(&table, "data")?,
                message: get_optional_value(&table, "message")?,
                source: get_optional_value(&table, "source")?,
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
                rendered: get_optional_value(&table, "rendered")?,
            };
            return Ok(out);
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "LspData".into(),
            message: Some(format!("expected a table got {value:?}")),
        })
    }
}

fn get_optional_value<T: FromLua>(table: &LuaTable, field: &str) -> mlua::Result<Option<T>> {
    match table.get::<Option<T>>(field) {
        Ok(out) => Ok(out),
        Err(LuaError::FromLuaConversionError { .. }) => Ok(None),
        e => e,
    }
}
