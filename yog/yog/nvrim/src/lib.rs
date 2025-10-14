//! Expose Rust helpers for my Nvim config to Lua via [`nvim_oxi`].
//!
//! Provide namespaced dictionaries for diagnostics, status UI (statusline / statuscolumn), CLI search flags,
//! buffer text, keymaps, colorscheme & style options, test running, and misc extensions.
//!
//! Each top‑level key is either:
//! - a table of related functions / data (e.g. `diagnostics`, `statusline`, `cli`)
//! - or a standalone function / value.

use nvim_oxi::Dictionary;

/// [`nvim_oxi::api::Buffer`] helpers.
mod buffer;
/// CLI flags for `fd` and `ripgrep`.
mod cli;
/// User commands.
mod cmds;
/// Colorscheme setup.
mod colorscheme;
/// Diagnostics filtering / formatting / sorting.
mod diagnostics;
/// Random string generation via the `fkr` crate.
mod fkr;
/// Core (non‑plugin) keymaps.
pub mod keymaps;
mod linters;
/// [`nvim_oxi`] custom extensions.
mod oxi_ext;
/// Status column (diagnostics + git signs).
mod statuscolumn;
/// Status line (diagnostics summary).
mod statusline;
/// Style options.
mod style_opts;
/// Text transform.
mod trex;
/// Rust tests utilities.
mod truster;
/// `vim.opts` utilities.
pub mod vim_opts;

/// Plugin entry point.
///
/// Returns a namespaced [`Dictionary`] whose values are grouped
/// sub‑dictionaries (diagnostics, UI, CLI flags, keymaps, etc.) plus a
/// few standalone helpers.
#[nvim_oxi::plugin]
fn nvrim() -> Dictionary {
    dict! {
        "diagnostics": diagnostics::dict(),
        "statusline": statusline::dict(),
        "statuscolumn": statuscolumn::dict(),
        "cmds": cmds::dict(),
        "cli": cli::dict(),
        "truster": truster::dict(),
        "buffer": buffer::dict(),
        "colorscheme": colorscheme::dict(),
        "style_opts": style_opts::dict(),
        "vim_opts": vim_opts::dict(),
        "keymaps": keymaps::dict(),
        "trex": trex::dict(),
        "fkr": fkr::dict(),
        "linters": linters::dict(),
    }
}
