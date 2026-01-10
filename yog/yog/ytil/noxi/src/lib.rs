//! Aggregated Nvim extension submodules.

pub mod buffer;
pub mod common;
pub mod dict;
pub mod extract;
pub mod inputlist;
pub mod jumplist;
pub mod macros;
pub mod mru_buffers;
pub mod notify;
pub mod quickfix;
pub mod tree_sitter;
pub mod vim_ui_select;
pub mod visual_selection;
pub mod window;

pub use nvim_oxi::*;
