use core::fmt::Debug;
use core::fmt::Display;
use std::rc::Rc;

use color_eyre::eyre::eyre;
use nvim_oxi::mlua;
use nvim_oxi::mlua::IntoLua;
use nvim_oxi::mlua::ObjectLike;

use crate::quickfix::QuickfixConfig;

/// Prompts the user to select an item from a list using Nvim's `vim.ui.select`.
///
/// Wraps the Lua `vim.ui.select` function to provide an interactive selection prompt.
/// The selected index (0-based) is passed to the provided callback.
///
/// # Arguments
/// - `choices` Iterable of displayable items to display for selection.
/// - `opts` Key-value pairs for additional options (e.g., prompt text).
/// - `callback` Closure invoked with the 0-based index of the selected choice.
///
/// # Returns
/// `Ok(())` if the selection succeeds.
///
/// # Errors
/// - Fails if `vim.ui.select` cannot be fetched from Lua globals.
/// - Fails if the options table cannot be created.
/// - Fails if calling `vim.ui.select` encounters an error.
pub fn open<C, K, V>(
    choices: impl IntoIterator<Item = C> + Debug,
    opts: &(impl IntoIterator<Item = (K, V)> + Debug + Clone),
    callback: impl Fn(usize) + 'static,
    maybe_quickfix: Option<QuickfixConfig>,
) -> color_eyre::Result<()>
where
    C: Display,
    K: IntoLua,
    V: IntoLua,
{
    let lua = mlua::lua();

    let vim_ui_select = lua
        .globals()
        .get_path::<mlua::Function>("vim.ui.select")
        .map_err(|err| eyre!("cannot fetch vim.ui.select function from Lua globals | error={err:#?}"))?;

    let opts_table = lua
        .create_table_from(opts.clone())
        .map_err(|err| eyre!("cannot create opts table | opts={opts:#?} error={err:#?}"))?;

    let quickfix = maybe_quickfix.map(Rc::new);

    let vim_ui_select_callback = lua
        .create_function(
            move |_: &mlua::Lua, (selected_value, idx): (Option<String>, Option<usize>)| {
                if let Some(quickfix) = &quickfix
                    && selected_value.is_some_and(|x| x == quickfix.trigger_value)
                {
                    let _ = crate::quickfix::open(quickfix.all_items.iter().map(|(s, i)| (s.as_str(), *i)))
                        .inspect_err(|err| {
                            crate::notify::error(format!("error opening quickfix | error={err:#?}"));
                        });
                    return Ok(());
                }
                if let Some(idx) = idx {
                    // The index passed to the callback is adjusted to take into account:
                    // - The 1-based indexing of the pickers
                    // - The additional quickfix synthetic entry in the picker
                    let adjusted_idx = if quickfix.is_some() { 2 } else { 1 };
                    callback(idx.saturating_sub(adjusted_idx));
                }
                Ok(())
            },
        )
        .map_err(|err| {
            eyre!("cannot create vim.ui.select callback | choices={choices:#?} opts={opts_table:#?} error={err:#?}")
        })?;

    let vim_ui_choices = choices.into_iter().map(|c| c.to_string()).collect::<Vec<_>>();

    vim_ui_select
        .call::<()>((vim_ui_choices.clone(), opts_table.clone(), vim_ui_select_callback))
        .map_err(|err| {
            eyre!("cannot call vim.ui.select | choices={vim_ui_choices:#?} opts={opts_table:#?} error={err:#?}")
        })?;

    Ok(())
}
