use std::ops::Range;

use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::GetTextOpts;
use nvim_oxi::api::types::ModeStr;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use serde::Deserialize;
use serde::Deserializer;

/// Get text in the current buffer between 2 positions.
///
/// Returned as a [`Vec`] of [`nvim_oxi::String`] lines suitable for Lua consumption.
/// On error, an empty [`Vec`] is returned and an error is notified.
///
/// Note: while porting this from Lua I discovered that multiple line visual selection cuts of
/// some characters at the start. Fortunately the multiple line visual selection is not yet used by
/// anyone. Only the single line selection is used to do live grep.
pub fn get_current((pos_1, pos_2): (GetPosOutput, GetPosOutput)) -> Vec<nvim_oxi::String> {
    let (line_rng, col_rng) = pos_1.ordered_line_and_col_ranges(&pos_2, &nvim_oxi::api::get_mode().mode);

    let cur_buf = Buffer::current();
    let Ok(lines) =  cur_buf
        .get_text(line_rng, col_rng.start, col_rng.end, &GetTextOpts::default())
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!(
                "cannot get text from buffer {cur_buf:#?} from start_pos {pos_1:#?} to end_pos {pos_2:#?}, error {error:#?}"
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
    pub fn ordered_line_and_col_ranges(&self, other: &Self, mode: &ModeStr) -> (Range<usize>, Range<usize>) {
        if self == other && mode == &"V" {
            return (self.lnum..self.lnum, self.col..usize::MAX);
        }
        let (start, end) = if self.lnum > other.lnum || self.col > other.col {
            (other, self)
        } else {
            (self, other)
        };
        ((start.lnum..end.lnum), (start.col..end.col))
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
