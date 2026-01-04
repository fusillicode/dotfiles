//! Utilities for user input selection from lists using Vimscript inputlist.

use core::fmt::Display;

use nvim_oxi::Array;

/// Prompt the user to select an item from a numbered list.
///
/// Displays `prompt` followed by numbered `items` via the Vimscript
/// `inputlist()` function and returns the chosen element (1-based user
/// index translated to 0-based). Returns [`None`] if the user cancels.
///
/// # Errors
/// - Invoking `inputlist()` fails.
/// - The returned index cannot be converted to `usize` (negative or overflow).
pub fn open<'a, I: Display>(prompt: &'a str, items: &'a [I]) -> color_eyre::Result<Option<&'a I>> {
    let displayable_items: Vec<_> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| format!("{}. {item}", idx.saturating_add(1)))
        .collect();

    let prompt_and_items = std::iter::once(prompt.to_string())
        .chain(displayable_items)
        .collect::<Array>();

    let idx = nvim_oxi::api::call_function::<_, i64>("inputlist", (prompt_and_items,))?;

    Ok(usize::try_from(idx.saturating_sub(1))
        .ok()
        .and_then(|idx| items.get(idx)))
}
