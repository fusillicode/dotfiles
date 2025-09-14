use nvim_oxi::Object;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Deserializer;
use serde::Deserialize;

/// Formats a diagnostic into a human-readable string.
#[allow(clippy::needless_pass_by_value)]
pub fn format(diagnostic: Diagnostic) -> Option<String> {
    let Some(msg) = get_msg(&diagnostic).map(|s| s.trim_end_matches('.').to_string()) else {
        crate::oxi_ext::api::notify_error(&format!("missing message in {diagnostic:#?}"));
        return None;
    };

    let Some(src) = get_src(&diagnostic).map(str::to_string) else {
        crate::oxi_ext::api::notify_error(&format!("missing source in {diagnostic:#?}"));
        return None;
    };

    let src_and_code = get_code(&diagnostic).map_or_else(|| src.clone(), |c| format!("{src}: {c}"));

    Some(format!("▶ {msg} [{src_and_code}]"))
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

/// Represents a diagnostic from Nvim.
#[derive(Debug, Deserialize)]
pub struct Diagnostic {
    /// The diagnostic code.
    code: Option<String>,
    /// The diagnostic message.
    message: Option<String>,
    /// The source of the diagnostic.
    source: Option<String>,
    /// Additional user data.
    user_data: Option<UserData>,
}

/// Implementation of [`FromObject`] for [`Diagnostic`].
impl FromObject for Diagnostic {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(Deserializer::new(obj)).map_err(Into::into)
    }
}

/// Implementation of [`Poppable`] for [`Diagnostic`].
impl Poppable for Diagnostic {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

/// User data associated with a diagnostic.
#[derive(Debug, Deserialize)]
pub struct UserData {
    /// LSP-specific diagnostic payload injected by Neovim
    lsp: Option<Lsp>,
}

/// LSP data within user data.
#[derive(Debug, Deserialize)]
pub struct Lsp {
    /// The diagnostic code.
    code: Option<String>,
    /// Additional LSP data.
    data: Option<LspData>,
    /// The diagnostic message.
    message: Option<String>,
    /// The source of the diagnostic.
    source: Option<String>,
}

/// Additional LSP data.
#[derive(Debug, Deserialize)]
pub struct LspData {
    rendered: Option<String>,
}
