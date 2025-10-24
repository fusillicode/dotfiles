//! Text transformation helpers for the current Visual selection.
//!
//! Provides a namespaced [`Dictionary`] exposing selection transformation
//! functionality (currently only case conversion via [`convert_case`]).

use convert_case::Case;
use convert_case::Casing as _;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;

use crate::dict;
use crate::fn_from;

/// Namespaced dictionary of text transform helpers.
///
/// Entries:
/// - `"transform_selection"`: wraps [`transform_selection`] and converts the active Visual selection to a user‑selected
///   [`Case`].
pub fn dict() -> Dictionary {
    dict! {
        "transform_selection": fn_from!(transform_selection),
    }
}

/// Transforms the current visual selection to a user-chosen case variant.
///
/// Prompts the user (via [`crate::oxi_ext::api::vim_ui_select`]) to select a case conversion
/// option, then applies the transformation to all selected lines in place.
///
/// Returns early if:
/// - No active Visual selection is detected.
/// - The user cancels the prompt.
/// - Writing the transformed text back to the buffer fails (an error is reported via
///   [`crate::oxi_ext::api::notify_error`]).
///
/// # Returns
/// Returns `Some(())` if the transformation succeeds, or `None` otherwise.
///
/// # Errors
/// Errors from [`crate::oxi_ext::api::vim_ui_select`] are reported via [`crate::oxi_ext::api::notify_error`]
/// using the direct display representation of [`color_eyre::Report`].
///
/// # Notes
/// Blockwise selections are treated as a contiguous span (not a rectangle).
pub fn transform_selection(_: ()) -> Option<()> {
    let selection = crate::oxi_ext::visual_selection::get(())?;

    let cases = Case::all_cases();
    let choices: Vec<String> = cases.iter().map(|c| format!("{c:?}")).collect();

    crate::oxi_ext::api::vim_ui_select(choices, [("prompt", "Select case ")], move |choice_idx| {
        if let Some(case) = cases.get(choice_idx) {
            let transformed_lines = selection
                .lines()
                .iter()
                .map(|line| line.as_str().to_case(*case))
                .collect::<Vec<_>>();
            if let Err(error) = Buffer::from(selection.buf_id()).set_text(
                selection.line_range(),
                selection.start().col,
                selection.end().col,
                transformed_lines,
            ) {
                crate::oxi_ext::api::notify_error(&format!(
                    "cannot set lines of buffer | start={:#?} end={:#?} error={error:#?}",
                    selection.start(),
                    selection.end()
                ));
            }
        }
    })
    .inspect_err(|error| {
        crate::oxi_ext::api::notify_error(&format!("{error}"));
    })
    .ok()?;

    None
}
