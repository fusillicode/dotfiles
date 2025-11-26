//! Custom Neovim plugins.

use nvim_oxi::Dictionary;

/// Scratch files selection and creation.
mod attempt;
/// Case conversion.
mod caseconv;
/// Random string generation via the [`fkr`] crate.
mod fkr;
/// Git diff line selection.
mod gdiff;
/// Generic text conversions.
mod genconv;
/// GitHub permalink generation for selected code.
mod ghurlinker;
/// Port of scrollofffraction.nvim plugin.
pub mod scrolloff;
/// Status column (diagnostics + git signs).
pub mod statuscolumn;
/// Status line (diagnostics summary).
pub mod statusline;
/// Nvim Terminal utilities
pub mod treminal;
/// Rust tests runner plugin.
pub mod truster;

pub fn dict() -> Dictionary {
    dict! {
        "attempt": attempt::dict(),
        "caseconv": caseconv::dict(),
        "fkr": fkr::dict(),
        "gdiff": gdiff::dict(),
        "genconv": genconv::dict(),
        "ghurlinker": ghurlinker::dict(),
        "statuscolumn": statuscolumn::dict(),
        "statusline": statusline::dict(),
        "treminal": treminal::dict(),
        "truster": truster::dict(),
    }
}
