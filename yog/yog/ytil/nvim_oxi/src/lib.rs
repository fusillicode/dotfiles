//! Aggregated Nvim extension submodules.
//!
//! Re‑exports helper modules (`api`, `buffer`, `dict`, `extract`, `macros`, `visual_selection`) used
//! throughout the plugin for structured Lua exposure.

pub mod buffer;
pub mod common;
pub mod dict;
pub mod extract;
pub mod inputlist;
pub mod macros;
pub mod notify;
pub mod quickfix;
pub mod vim_ui_select;
pub mod visual_selection;

pub use nvim_oxi::*;
