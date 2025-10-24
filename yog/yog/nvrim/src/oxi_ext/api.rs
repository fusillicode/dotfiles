//! Thin wrappers over common Neovim API functions with improved error reporting.
//!
//! Helpers include global var setting, notifications (`notify_error` / `notify_warn`), ex command
//! execution, and interactive list selection (`inputlist`).

use core::fmt::Debug;

use color_eyre::eyre::eyre;
use nvim_oxi::Array;
use nvim_oxi::api::opts::CmdOpts;
use nvim_oxi::api::types::CmdInfosBuilder;
use nvim_oxi::api::types::LogLevel;
use nvim_oxi::conversion::ToObject;
use nvim_oxi::mlua;
use nvim_oxi::mlua::IntoLua;
use nvim_oxi::mlua::ObjectLike;

use crate::dict;

/// Sets the value of a global Nvim variable `name` to `value`.
///
/// Wraps [`nvim_oxi::api::set_var`].
///
/// Errors are reported to Nvim via [`notify_error`].
pub fn set_g_var<V: ToObject + Debug>(name: &str, value: V) {
    let msg = format!("cannot set global var | name={name} value={value:#?}");
    if let Err(error) = nvim_oxi::api::set_var(name, value) {
        crate::oxi_ext::api::notify_error(&format!("{msg} | error={error:#?}"));
    }
}

/// Notifies the user of an error message in Nvim.
pub fn notify_error(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Error, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify error | msg={msg:?} error={error:#?}"));
    }
}

/// Notifies the user of a warning message in Nvim.
pub fn notify_warn(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Warn, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify warning | msg={msg:?} error={error:#?}"));
    }
}

/// Execute an ex command with arguments.
///
/// Wraps [`nvim_oxi::api::cmd`], reporting failures through
/// [`crate::oxi_ext::api::notify_error`].
pub fn exec_vim_cmd<S, I>(cmd: impl Into<String> + Debug + std::marker::Copy, args: I)
where
    S: Into<String>,
    I: IntoIterator<Item = S> + Debug + std::marker::Copy,
{
    if let Err(error) = nvim_oxi::api::cmd(
        &CmdInfosBuilder::default().cmd(cmd).args(args).build(),
        &CmdOpts::default(),
    ) {
        crate::oxi_ext::api::notify_error(&format!(
            "cannot execute cmd | cmd={cmd:?} args={args:#?} error={error:#?}"
        ));
    }
}

/// Prompt the user to select an item from a numbered list.
///
/// Displays `prompt` followed by numbered `items` via the Vimscript
/// `inputlist()` function and returns the chosen element (1-based user
/// index translated to 0-based). Returns [`None`] if the user cancels.
///
/// # Arguments
/// - `prompt`: Heading line shown before the options.
/// - `items`: Slice of displayable values listed sequentially.
///
/// # Errors
/// - Invoking `inputlist()` fails.
/// - The returned index cannot be converted to `usize` (negative or overflow).
pub fn inputlist<'a, I: core::fmt::Display>(prompt: &'a str, items: &'a [I]) -> color_eyre::Result<Option<&'a I>> {
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

/// Prompts the user to select an item from a list using Neovim's `vim.ui.select`.
///
/// Wraps the Lua `vim.ui.select` function to provide an interactive selection prompt.
/// The selected index (0-based) is passed to the provided callback.
///
/// # Arguments
/// - `choices` List of string options to display for selection.
/// - `opts` Key-value pairs for additional options (e.g., prompt text).
/// - `callback` Closure invoked with the 0-based index of the selected choice.
///
/// # Returns
/// Returns `Ok(())` if the selection succeeds.
///
/// # Errors
/// - Fails if `vim.ui.select` cannot be fetched from Lua globals.
/// - Fails if the options table cannot be created.
/// - Fails if calling `vim.ui.select` encounters an error.
pub fn vim_ui_select<K, V>(
    choices: &[String],
    opts: &(impl IntoIterator<Item = (K, V)> + Debug + Clone),
    callback: impl Fn(usize) + 'static,
) -> color_eyre::Result<()>
where
    K: IntoLua,
    V: IntoLua,
{
    let lua = mlua::lua();

    let vim_ui_select = lua
        .globals()
        .get_path::<mlua::Function>("vim.ui.select")
        .map_err(|error| eyre!("cannot fetch vim.ui.select function from Lua globals | error={error:#?}"))?;

    let opts_table = lua
        .create_table_from(opts.clone())
        .map_err(|error| eyre!("cannot create opts table | opts={opts:#?} error={error:#?}"))?;

    let vim_ui_select_callback = lua
        .create_function(move |_: &mlua::Lua, (_, idx1): (Option<String>, Option<usize>)| {
            if let Some(idx) = idx1.map(|idx1| idx1.saturating_sub(1)) {
                callback(idx);
            }
            Ok(())
        })
        .map_err(|error| {
            eyre!("cannot create vim.ui.select callback | choices={choices:#?} opts={opts_table:#?} error={error:#?}")
        })?;

    vim_ui_select
        .call::<()>((choices.to_owned(), opts_table.clone(), vim_ui_select_callback))
        .map_err(|error| {
            eyre!("cannot call vim.ui.select | choices={choices:#?} opts={opts_table:#?} error={error:#?}")
        })?;

    Ok(())
}
