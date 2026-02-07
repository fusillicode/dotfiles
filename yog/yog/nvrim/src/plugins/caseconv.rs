//! Text conversions helpers for the current Visual selection.
//!
//! Provides a namespaced [`Dictionary`] exposing selection conversion
//! functionality (currently only case conversion via [`convert_case`]).

use core::fmt::Display;

use convert_case::Case;
use convert_case::Casing as _;
use nvim_oxi::Dictionary;
use rootcause::Report;

/// Namespaced dictionary of case conversion helpers.
///
/// Entries:
/// - `"convert_selection"`: wraps [`convert_selection`] and converts the active Visual selection to a user‑selected
///   [`Case`].
pub fn dict() -> Dictionary {
    dict! {
        "convert_selection": fn_from!(convert_selection),
    }
}

/// Converts the current visual selection to a user-chosen case variant.
///
/// Prompts the user (via [`ytil_noxi::vim_ui_select::open`]) to select a case conversion
/// option, then applies the conversion to all selected lines in place.
///
/// Returns early if:
/// - No active Visual selection is detected.
/// - The user cancels the prompt.
/// - Writing the converted text back to the buffer fails (an error is reported via [`ytil_noxi::notify::error`]).
///
/// # Errors
/// Errors from [`ytil_noxi::vim_ui_select::open`] are reported via [`ytil_noxi::notify::error`]
/// using the direct display representation of [`rootcause::Report`].
///
/// # Notes
/// Blockwise selections are treated as a contiguous span (not a rectangle).
fn convert_selection(_: ()) {
    let Some(selection) = ytil_noxi::visual_selection::get(()) else {
        return;
    };

    let cases = Case::all_cases();

    let callback = move |choice_idx| {
        cases.get(choice_idx).map(|case| {
            let converted_lines = selection
                .lines()
                .iter()
                .map(|line| line.as_str().to_case(*case))
                .collect::<Vec<_>>();
            ytil_noxi::buffer::replace_text_and_notify_if_error(&selection, converted_lines);
            Ok::<(), Report>(())
        });
    };

    if let Err(err) = ytil_noxi::vim_ui_select::open(
        cases.iter().map(DisplayableCase),
        &[("prompt", "Convert selection to case ")],
        callback,
        None,
    ) {
        ytil_noxi::notify::error(format!("error converting selection to case | error={err:#?}"));
    }
}

/// Newtype wrapper to make [`Case`] displayable using its [`Debug`] representation.
struct DisplayableCase<'a>(&'a Case<'a>);

impl Display for DisplayableCase<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
