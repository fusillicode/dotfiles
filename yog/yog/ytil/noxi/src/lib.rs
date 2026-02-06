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

// Selective re-exports of nvim_oxi types used by downstream crates (e.g. nvrim).
// Avoids `pub use nvim_oxi::*` which would make every upstream breaking change silently propagate.
pub use nvim_oxi::Dictionary;
pub use nvim_oxi::api;
pub use nvim_oxi::plugin;
