//! Neovim jumplist utilities for accessing jump history.

use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use serde::Deserialize;

/// Represents a single entry in Neovim's jumplist.
#[derive(Clone, Debug, Deserialize)]
pub struct JumpEntry {
    pub bufnr: i32,
    pub col: i32,
    pub coladd: i32,
    pub lnum: i32,
}

impl FromObject for JumpList {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(nvim_oxi::serde::Deserializer::new(obj)).map_err(Into::into)
    }
}

impl Poppable for JumpList {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

/// Internal representation of Neovim's jumplist structure.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JumpList(Vec<JumpEntry>, usize);

/// Retrieves the current jumplist from Neovim.
pub fn get() -> Option<Vec<JumpEntry>> {
    Some(
        nvim_oxi::api::call_function::<_, JumpList>("getjumplist", Array::new())
            .inspect_err(|err| crate::notify::error(format!("error getting jumplist | error={err:?}")))
            .ok()?
            .0,
    )
}
