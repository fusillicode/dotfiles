//! Text transformation helpers for the current Visual selection.
//!
//! Provides a namespaced [`Dictionary`] exposing selection transformation
//! functionality (currently only case conversion via [`convert_case`]).

use convert_case::Case;
use convert_case::Casing as _;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::mlua;
use nvim_oxi::mlua::ObjectLike;

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
/// Prompts (via [`crate::oxi_ext::api::inputlist`]) for a case variant, converts all
/// selected lines in place, and replaces the selection text. Returns early if:
/// - No active Visual selection is detected.
/// - The user cancels the prompt.
/// - Writing the transformed text back to the buffer fails (an error is reported via
///   [`crate::oxi_ext::api::notify_error`]).
///
/// # Notes
/// Blockwise selections are treated as a contiguous span (not a rectangle).
pub fn transform_selection(_: ()) -> Option<()> {
    let selection = crate::oxi_ext::visual_selection::get(())?;

    let choices: Vec<String> = Case::all_cases().iter().map(|c| format!("{:?}", c)).collect();

    let lua = mlua::lua();
    let opts = [("prompt", "Select case ")];
    let opts = lua
        .create_table_from(opts)
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!("cannot create opts table | opts={opts:#?} error={error:#?}",));
        })
        .ok()?;

    let callback = lua
        .create_function(move |_: &mlua::Lua, (_, idx1): (Option<String>, Option<usize>)| {
            if let Some(idx) = idx1.map(|idx1| idx1.saturating_sub(1))
                && let Some(case) = Case::all_cases().get(idx)
            {
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
            Ok(())
        })
        .unwrap();

    let vim_ui_select = lua
        .globals()
        .get_path::<mlua::Function>("vim.ui.select")
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "error fetching vim.ui.select function from Lua globals | error={error:#?}",
            ));
        })
        .ok()?;

    vim_ui_select
        .call::<()>((choices.clone(), opts.clone(), callback))
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "error calling vim.ui.select | choices={choices:#?} opts={opts:#?} error={error:#?}",
            ));
        })
        .ok()?;

    None
}
