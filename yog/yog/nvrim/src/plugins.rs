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
/// Rust tests utilities.
pub mod truster;

pub fn dict() -> Dictionary {
    dict! {
        "statusline": statusline::dict(),
        "statuscolumn": statuscolumn::dict(),
        "truster": truster::dict(),
        "caseconv": caseconv::dict(),
        "fkr": fkr::dict(),
        "attempt": attempt::dict(),
        "genconv": genconv::dict(),
        "ghurlinker": ghurlinker::dict(),
        "gdiff": gdiff::dict(),
    }
}
