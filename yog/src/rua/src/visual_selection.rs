use itertools::Itertools;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::GetTextOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use serde::Deserialize;
use serde::Deserializer;

/// Get selected text between two [`GetPosOutput`] positions.
///
/// Returns [`Some<String>`] on success, [`None`] on error.
pub fn get((start_pos, end_pos): (GetPosOutput, GetPosOutput)) -> Option<String> {
    let start_ln = core::cmp::min(start_pos.lnum, end_pos.lnum);
    let end_ln = core::cmp::max(start_pos.lnum, end_pos.lnum);
    let start_col = core::cmp::min(start_pos.col, end_pos.col);
    let end_col = core::cmp::max(start_pos.col, end_pos.col);

    let cur_buf = Buffer::current();
    let selected_text = cur_buf
        .get_text(start_ln..end_ln, start_col, end_col, &GetTextOpts::default())
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!(
                "cannot get selected text from buffer {cur_buf:#?}, start_pos {start_pos:#?}, end_pos {end_pos:#?} error {error:#?}"
            ));
        })
        .ok()?
        // To avoid extra allocation via .to_string after .to_string_lossy
        .format_with("\n", |oxi_string, fmt|
            fmt(&oxi_string.to_string_lossy())
        ).to_string();

    Some(selected_text)
}

/// Normalized output of Neovim `getpos()` for visual selections.
///
/// Built from [`GetPosRaw`].
#[derive(Debug, Clone, Copy)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
pub struct GetPosOutput {
    pub bufnum: i64,
    pub lnum: usize,
    pub col: usize,
    pub off: i64,
}

/// Custom [`Deserialize`] from Lua tuple (see [`GetPosRaw`]).
impl<'de> Deserialize<'de> for GetPosOutput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let t = GetPosRaw::deserialize(deserializer)?;
        Ok(Self::from(t))
    }
}

/// Convert [`GetPosRaw`] to [`GetPosOutput`], switching 1-based to 0-based indices.
impl From<GetPosRaw> for GetPosOutput {
    fn from(raw: GetPosRaw) -> Self {
        fn from_lua_idx_to_usize(v: i64) -> usize {
            usize::try_from(v.saturating_sub(1)).unwrap_or_else(|_| usize::default())
        }

        Self {
            bufnum: raw.0,
            lnum: from_lua_idx_to_usize(raw.1),
            col: from_lua_idx_to_usize(raw.2),
            off: raw.3,
        }
    }
}

/// Raw `getpos()` tuple: (`bufnum`, `lnum`, `col`, `off`).
#[derive(Debug, Clone, Copy, Deserialize)]
struct GetPosRaw(i64, i64, i64, i64);

/// Implementation of [`FromObject`] for [`GetPosOutput`].
impl FromObject for GetPosOutput {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(nvim_oxi::serde::Deserializer::new(obj)).map_err(Into::into)
    }
}

/// Implementation of [`Poppable`] for [`GetPosOutput`].
impl Poppable for GetPosOutput {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}
