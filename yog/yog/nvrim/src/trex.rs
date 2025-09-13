//! Text transformation helpers for the current Visual selection.
//!
//! Provides a namespaced [`Dictionary`] exposing selection transformation
//! functionality (currently only case conversion via [`convert_case`]).

use std::ops::Deref;

use convert_case::Case;
use convert_case::Casing as _;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;

use crate::buffer::visual_selection;
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

/// Transform the current Visual selection to a user‑chosen [`Case`].
///
/// Prompts (via [`crate::oxi_ext::inputlist`]) for a case variant, converts all
/// selected lines in place, and replaces the selection text. Returns early if:
/// - No active Visual selection is detected.
/// - The user cancels the prompt.
/// - Writing the transformed text back to the buffer fails (an error is reported via [`crate::oxi_ext::notify_error`]).
///
/// # Notes
///
/// Blockwise selections are treated as a contiguous span (not a rectangle).
pub fn transform_selection(_: ()) {
    let Some(selection) = visual_selection::get(()) else {
        return;
    };

    let options: Vec<_> = Case::all_cases().iter().copied().map(CaseWrap).collect();
    let Ok(selected_option) = crate::oxi_ext::inputlist("Select option:", &options).inspect_err(|error| {
        crate::oxi_ext::notify_error(&format!("cannot get user input, error {error:#?}"));
    }) else {
        return;
    };
    let Some(selected_option) = selected_option else {
        return;
    };

    let transformed_lines = selection
        .lines()
        .iter()
        .map(|line| line.as_str().to_case(**selected_option))
        .collect::<Vec<_>>();

    if let Err(error) = Buffer::from(selection.buf_id()).set_text(
        selection.line_range(),
        selection.start().col,
        selection.end().col,
        transformed_lines,
    ) {
        crate::oxi_ext::notify_error(&format!(
            "cannot set lines of buffer between {:#?} and {:#?}, error {error:#?}",
            selection.start(),
            selection.end()
        ));
    }
}

/// Wrapper implementing [`core::fmt::Display`] for [`Case`] so choices can be
/// shown in the prompt list.
struct CaseWrap<'a>(Case<'a>);

impl core::fmt::Display for CaseWrap<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<'a> Deref for CaseWrap<'a> {
    type Target = Case<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
