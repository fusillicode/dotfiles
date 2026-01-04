//! Neovim jumplist utilities for accessing jump history.
//!
//! Provides types and functions to retrieve and parse the Neovim jumplist,
//! which contains the history of cursor positions jumped to using commands
//! like `Ctrl-I`, `Ctrl-O`, `:tag`, etc. The jumplist is represented as a
//! vector of jump entries with an associated current position index.

use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use serde::Deserialize;

/// Represents a single entry in Neovim's jumplist.
///
/// Contains the cursor position and buffer information for a specific jump
/// location. All coordinates use Nvim's conventions (1-based for lines, 0-based
/// for columns unless otherwise specified).
///
/// # Assumptions
/// - Coordinates follow Nvim's internal conventions.
/// - `coladd` is used for multi-byte character positioning in some contexts.
///
/// # Rationale
/// Direct mapping of Nvim's jumplist entry structure to enable seamless
/// conversion from Lua API responses.
#[derive(Clone, Debug, Deserialize)]
pub struct JumpEntry {
    /// Buffer number where the jump occurred.
    pub bufnr: i32,
    /// Column position (0-based byte offset).
    pub col: i32,
    /// Column addition for virtual cursor positioning.
    pub coladd: i32,
    /// Line number (1-based).
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
///
/// Wraps the raw jumplist data returned by Nvim's `getjumplist()` function,
/// which consists of a vector of jump entries and the current position index.
/// This type is used internally for deserialization and conversion.
///
/// # Fields
/// - `Vec<JumpEntry>` All jump entries and the usize represents.
/// - `usize` The current position index in the jumplist.
///
/// # Rationale
/// Kept private to expose only the jump entries vector via the public API,
/// hiding implementation details about position tracking.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JumpList(Vec<JumpEntry>, usize);

/// Retrieves the current jumplist from Neovim.
///
/// Calls Nvim's `getjumplist()` function and extracts the jump entries vector.
/// Errors are handled internally by notifying Nvim and returning `None`.
///
/// # Errors
/// - Lua function call fails when invoking `getjumplist`.
/// - Deserialization fails when parsing the jumplist structure.
/// - All errors are notified to Nvim via [`crate::notify::error`] and converted to `None`.
pub fn get() -> Option<Vec<JumpEntry>> {
    Some(
        nvim_oxi::api::call_function::<_, JumpList>("getjumplist", Array::new())
            .inspect_err(|err| crate::notify::error(format!("error getting jumplist | error={err:?}")))
            .ok()?
            .0,
    )
}
