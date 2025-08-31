use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::GetTextOpts;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use serde::Deserialize;
use serde::Deserializer;

use crate::oxi_ext::BufferExt;

/// Return text from the current buffer between two positions.
///
/// Produces a [`Vec`] of [`nvim_oxi::String`] lines for Lua.
/// On error, returns an empty [`Vec`] and emits a notification to Neovim.
pub fn get_current((pos1, pos2): (GetPosOutput, GetPosOutput)) -> Vec<nvim_oxi::String> {
    let (start_pos, end_pos) = pos1.switch_if_needed(pos2);
    let cur_buf = Buffer::current();

    let end_col = if start_pos == end_pos && nvim_oxi::api::get_mode().mode == "V" {
        cur_buf
            .get_line(start_pos.lnum)
            .map(|line| line.len())
            .inspect_err(|error| {
                crate::oxi_ext::notify_error(&format!(
                    "cannot get buffer line with idx {} from buffer {cur_buf:#?}, error {error:#?}",
                    start_pos.lnum
                ));
            })
            .unwrap_or(start_pos.col)
    } else {
        // To fix missing last char selection
        end_pos.col.saturating_add(1)
    };

    let Ok(lines) =  cur_buf
        .get_text(start_pos.lnum..end_pos.lnum, start_pos.col, end_col, &GetTextOpts::default())
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!(
                "cannot get text from buffer {cur_buf:#?} from start_pos {start_pos:#?} to end_pos {end_pos:#?}, error {error:#?}"
            ));
        }) else {
            return vec![];
        };

    lines.collect()
}

/// Normalized, 0-based indexed output of Neovim `getpos()`.
///
/// Built from [`GetPosRaw`].
#[derive(Debug, PartialEq, Eq)]
pub struct GetPosOutput {
    pub lnum: usize,
    pub col: usize,
}

impl GetPosOutput {
    pub const fn switch_if_needed(self, other: Self) -> (Self, Self) {
        if self.lnum > other.lnum || self.col > other.col {
            (other, self)
        } else {
            (self, other)
        }
    }
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

/// Convert [`GetPosRaw`] to [`GetPosOutput`] by switching to 0-based indexing from Lua 1-based.
impl From<GetPosRaw> for GetPosOutput {
    fn from(raw: GetPosRaw) -> Self {
        fn to_0_based_usize(v: i64) -> usize {
            usize::try_from(v.saturating_sub(1)).unwrap_or(usize::MIN)
        }

        Self {
            lnum: to_0_based_usize(raw.1),
            col: to_0_based_usize(raw.2),
        }
    }
}

/// Raw `getpos()` tuple: (`bufnum`, `lnum`, `col`, `off`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
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
