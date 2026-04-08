//! Custom Neovim plugins.

use nvim_oxi::Dictionary;

/// Scratch files selection and creation.
mod attempt;
/// Case conversion.
mod caseconv;
/// Close-other-buffers helpers.
pub mod clotherbufs;
/// Random string generation via the [`fkr`] crate.
mod fkr;
/// `fzf-lua` integration helpers.
mod fzf_lua;
/// Git diff line selection.
mod gdiff;
/// Generic text conversions.
mod genconv;
/// GitHub permalink generation for selected code.
mod ghurlinker;
/// Open/copy/reveal helpers for paths and symbols.
pub mod opener;
/// Port of scrollofffraction.nvim plugin.
pub mod scrolloff;
/// Status column (diagnostics + git signs).
pub mod statuscolumn;
/// Status line (diagnostics summary).
pub mod statusline;
/// Rust tests runner plugin.
pub mod truster;

pub fn dict() -> Dictionary {
    dict! {
        "attempt": attempt::dict(),
        "caseconv": caseconv::dict(),
        "clotherbufs": clotherbufs::dict(),
        "fkr": fkr::dict(),
        "fzf_lua": fzf_lua::dict(),
        "gdiff": gdiff::dict(),
        "genconv": genconv::dict(),
        "ghurlinker": ghurlinker::dict(),
        "opener": opener::dict(),
        "statuscolumn": statuscolumn::dict(),
        "statusline": statusline::dict(),
        "truster": truster::dict(),
    }
}
