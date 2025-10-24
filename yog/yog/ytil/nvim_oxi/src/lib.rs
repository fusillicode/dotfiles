//! Aggregated Neovim extension submodules.
//!
//! Re‑exports helper modules (`api`, `buffer`, `dict`, `extract`, `macros`, `visual_selection`) used
//! throughout the plugin for structured Lua exposure.

pub mod api;
pub mod buffer;
pub mod dict;
pub mod extract;
pub mod macros;
pub mod visual_selection;

pub use nvim_oxi::*;
