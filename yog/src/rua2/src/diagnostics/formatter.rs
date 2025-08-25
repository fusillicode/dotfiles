use nvim_oxi::Function;
use nvim_oxi::Object;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::conversion::ToObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::Pushable;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Deserializer;
use nvim_oxi::serde::Serializer;
use serde::Deserialize;
use serde::Serialize;

pub fn format() -> Object {
    Object::from(Function::<Diagnostic, nvim_oxi::Result<_>>::from_fn(format_core))
}

fn format_core(diagnostic: Diagnostic) -> nvim_oxi::Result<String> {
    let msg = get_msg(&diagnostic).map_or_else(
        || format!("no message in {diagnostic:#?}"),
        |s| s.trim_end_matches('.').to_string(),
    );
    let src = get_src(&diagnostic).map_or_else(|| format!("no source in {diagnostic:#?}"), str::to_string);
    let code = get_code(&diagnostic);
    let src_and_code = code.map_or_else(|| src.clone(), |c| format!("{src}: {c}"));

    Ok(format!("▶ {msg} [{src_and_code}]"))
}

/// Extracts LSP diagnostic message from [LspData::rendered] or directly from the supplied [Diagnostic].
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

/// Extracts the "source" from [Diagnostic::user_data] or [Diagnostic::source].
fn get_src(diag: &Diagnostic) -> Option<&str> {
    diag.user_data
        .as_ref()
        .and_then(|user_data| user_data.lsp.as_ref().and_then(|lsp| lsp.source.as_deref()))
        .or(diag.source.as_deref())
}

/// Extracts the "code" from [Diagnostic::user_data] or [Diagnostic::code].
fn get_code(diag: &Diagnostic) -> Option<&str> {
    diag.user_data
        .as_ref()
        .and_then(|user_data| user_data.lsp.as_ref().and_then(|lsp| lsp.code.as_deref()))
        .or(diag.code.as_deref())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    code: Option<String>,
    message: Option<String>,
    source: Option<String>,
    user_data: Option<UserData>,
}

impl FromObject for Diagnostic {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(Deserializer::new(obj)).map_err(Into::into)
    }
}

impl ToObject for Diagnostic {
    fn to_object(self) -> Result<Object, nvim_oxi::conversion::Error> {
        self.serialize(Serializer::new()).map_err(Into::into)
    }
}

impl Poppable for Diagnostic {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

impl Pushable for Diagnostic {
    unsafe fn push(self, lstate: *mut State) -> Result<std::ffi::c_int, nvim_oxi::lua::Error> {
        unsafe {
            self.to_object()
                .map_err(nvim_oxi::lua::Error::push_error_from_err::<Self, _>)?
                .push(lstate)
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserData {
    lsp: Option<Lsp>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Lsp {
    code: Option<String>,
    data: Option<LspData>,
    message: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LspData {
    rendered: Option<String>,
}
