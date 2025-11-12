//! Expose Rust helpers for my Nvim config to Lua via [`nvim_oxi`].
//!
//! Provide namespaced dictionaries for diagnostics, status UI (statusline / statuscolumn), CLI search flags,
//! buffer text, keymaps, colorscheme & style options, test running, and misc extensions.
//!
//! Each top‑level key is either:
//! - a table of related functions / data (e.g. `diagnostics`, `statusline`, `cli`)
//! - or a standalone function / value.

use ytil_nvim_oxi::Dictionary;

#[macro_use]
mod macros;

/// Scratch files selection and creation.
mod attempt;
/// [`nvim_oxi::api::Buffer`] helpers.
mod buffer;
/// Case conversion.
mod caseconv;
/// CLI flags for `fd` and `ripgrep`.
mod cli;
/// User commands.
mod cmds;
/// Colorscheme setup.
mod colorscheme;
/// Diagnostics filtering / formatting / sorting.
mod diagnostics;
/// Random string generation via the [`fkr`] crate.
mod fkr;
/// Generic text conversions.
mod genconv;
mod gitlinker;
/// Core (non‑plugin) keymaps.
pub mod keymaps;
/// Utilities to handle linters output
mod linters;
/// Port of scrollofffraction.nvim plugin.
mod scrolloff;
/// Status column (diagnostics + git signs).
mod statuscolumn;
/// Status line (diagnostics summary).
mod statusline;
/// Style options.
mod style_opts;
/// Rust tests utilities.
mod truster;
/// `vim.opts` utilities. Avoids intra-doc links to private items for stable docs; uses plain function calls for error
/// notifications.
pub mod vim_opts;

/// Plugin entry point.
///
/// Returns a namespaced [`Dictionary`] whose values are grouped
/// sub‑dictionaries (diagnostics, UI, CLI flags, keymaps, etc.) plus a
/// few standalone helpers.
#[ytil_nvim_oxi::plugin]
fn nvrim() -> Dictionary {
    ytil_nvim_oxi::dict! {
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
        "caseconv": caseconv::dict(),
        "fkr": fkr::dict(),
        "linters": linters::dict(),
        "attempt": attempt::dict(),
        "genconv": genconv::dict(),
        "gitlinker": gitlinker::dict(),
    }
}
