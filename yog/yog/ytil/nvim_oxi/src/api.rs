//! Thin wrappers over common Neovim API functions with improved error reporting.
//!
//! Helpers include global var setting, notifications (`notify_error` / `notify_warn`), ex command
//! execution, and interactive list selection (`inputlist`).

use core::fmt::Debug;
use core::fmt::Display;

use color_eyre::eyre::eyre;
use nvim_oxi::Array;
pub use nvim_oxi::api::opts;
use nvim_oxi::api::opts::CmdOpts;
pub use nvim_oxi::api::types;
use nvim_oxi::api::types::CmdInfosBuilder;
use nvim_oxi::api::types::LogLevel;
use nvim_oxi::conversion::ToObject;
use nvim_oxi::mlua;
use nvim_oxi::mlua::IntoLua;
use nvim_oxi::mlua::ObjectLike;

use crate::dict;

/// Types that can be converted to a notification message for Neovim.
///
/// Implementors provide a way to transform themselves into a string suitable for display
/// in Neovim notifications.
///
/// # Returns
/// A string representation of the notifiable item.
pub trait Notifiable: Debug {
    fn to_msg(&self) -> impl AsRef<str>;
}

impl Notifiable for color_eyre::Report {
    fn to_msg(&self) -> impl AsRef<str> {
        self.to_string()
    }
}

impl Notifiable for &color_eyre::Report {
    fn to_msg(&self) -> impl AsRef<str> {
        self.to_string()
    }
}

impl Notifiable for String {
    fn to_msg(&self) -> impl AsRef<str> {
        self
    }
}

impl Notifiable for &str {
    fn to_msg(&self) -> impl AsRef<str> {
        self
    }
}

/// Sets the value of a global Nvim variable `name` to `value`.
///
/// Wraps [`nvim_oxi::api::set_var`].
///
/// Errors are reported to Nvim via [`notify_error`].
pub fn set_g_var<V: ToObject + Debug>(name: &str, value: V) {
    let msg = format!("cannot set global var | name={name} value={value:#?}");
    if let Err(error) = nvim_oxi::api::set_var(name, value) {
        crate::api::notify_error(format!("{msg} | error={error:#?}"));
    }
}

/// Notifies the user of an error message in Nvim.
#[allow(clippy::needless_pass_by_value)]
pub fn notify_error<N: Notifiable>(notifiable: N) {
    if let Err(error) = nvim_oxi::api::notify(notifiable.to_msg().as_ref(), LogLevel::Error, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify error | msg={notifiable:?} error={error:#?}"));
    }
}

/// Notifies the user of a warning message in Nvim.
#[allow(clippy::needless_pass_by_value)]
pub fn notify_warn<N: Notifiable>(notifiable: N) {
    if let Err(error) = nvim_oxi::api::notify(notifiable.to_msg().as_ref(), LogLevel::Warn, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify warning | msg={notifiable:?} error={error:#?}"));
    }
}

/// Execute an ex command with optional arguments.
///
/// Wraps [`nvim_oxi::api::cmd`], reporting failures through [`crate::api::notify_error`].
///
/// # Arguments
/// - `cmd` The ex command to execute.
/// - `args` Optional list of arguments for the command.
///
/// # Returns
/// Returns `Ok(output)` where `output` is the command's output if any, or `Err(error)` if execution failed.
///
/// # Errors
/// Errors from [`nvim_oxi::api::cmd`] are propagated after logging via [`crate::api::notify_error`].
pub fn exec_vim_cmd(
    cmd: impl AsRef<str> + Debug + std::marker::Copy,
    args: Option<&[impl AsRef<str> + Debug]>,
) -> Result<Option<String>, nvim_oxi::api::Error> {
    let mut cmd_infos_builder = CmdInfosBuilder::default();
    cmd_infos_builder.cmd(cmd.as_ref());
    if let Some(args) = args {
        cmd_infos_builder.args(args.iter().map(|s| s.as_ref().to_string()));
    }
    nvim_oxi::api::cmd(&cmd_infos_builder.build(), &CmdOpts::default()).inspect_err(|error| {
        crate::api::notify_error(format!(
            "cannot execute cmd | cmd={cmd:?} args={args:#?} error={error:#?}",
        ));
    })
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
pub fn inputlist<'a, I: Display>(prompt: &'a str, items: &'a [I]) -> color_eyre::Result<Option<&'a I>> {
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
pub fn vim_ui_select<C, K, V>(
    choices: impl IntoIterator<Item = C> + Debug,
    opts: &(impl IntoIterator<Item = (K, V)> + Debug + Clone),
    callback: impl Fn(usize) + 'static,
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

    let vim_ui_choices = choices.into_iter().map(|c| c.to_string()).collect::<Vec<_>>();

    vim_ui_select
        .call::<()>((vim_ui_choices.clone(), opts_table.clone(), vim_ui_select_callback))
        .map_err(|error| {
            eyre!("cannot call vim.ui.select | choices={vim_ui_choices:#?} opts={opts_table:#?} error={error:#?}")
        })?;

    Ok(())
}
