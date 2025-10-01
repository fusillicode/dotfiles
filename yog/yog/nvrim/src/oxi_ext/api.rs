use nvim_oxi::Array;
use nvim_oxi::api::opts::CmdOpts;
use nvim_oxi::api::types::CmdInfosBuilder;
use nvim_oxi::api::types::LogLevel;
use nvim_oxi::conversion::ToObject;

use crate::dict;

/// Sets the value of a global Nvim variable `name` to `value`.
///
/// Wraps [`nvim_oxi::api::set_var`].
///
/// Errors are reported to Nvim via [`notify_error`].
pub fn set_g_var<V: ToObject + core::fmt::Debug>(name: &str, value: V) {
    let msg = format!("cannot set global var {name} value {value:#?}");
    if let Err(error) = nvim_oxi::api::set_var(name, value) {
        crate::oxi_ext::api::notify_error(&format!("{msg}, error {error:#?}"));
    }
}

/// Notifies the user of an error message in Nvim.
pub fn notify_error(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Error, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify error {msg:?}, error {error:#?}"));
    }
}

/// Notifies the user of a warning message in Nvim.
#[expect(dead_code, reason = "Kept for future use")]
pub fn notify_warn(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Warn, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify warning {msg:?}, error {error:#?}"));
    }
}

/// Execute an ex command with arguments.
///
/// Wraps [`nvim_oxi::api::cmd`], reporting failures through
/// [`crate::oxi_ext::api::notify_error`].
pub fn exec_vim_cmd<S, I>(cmd: impl Into<String> + core::fmt::Debug + std::marker::Copy, args: I)
where
    S: Into<String>,
    I: IntoIterator<Item = S> + core::fmt::Debug + std::marker::Copy,
{
    if let Err(error) = nvim_oxi::api::cmd(
        &CmdInfosBuilder::default().cmd(cmd).args(args).build(),
        &CmdOpts::default(),
    ) {
        crate::oxi_ext::api::notify_error(&format!(
            "cannot execute cmd {cmd:?} with args {args:#?}, error {error:#?}"
        ));
    }
}

/// Prompt the user to select an item from a numbered list.
///
/// Displays `prompt` followed by numbered `items` via the Vimscript
/// `inputlist()` function and returns the chosen element (1-based user
/// index translated to 0-based). Returns [`None`] if the user cancels.
///
/// # Parameters
/// - `prompt`: Heading line shown before the options.
/// - `items`: Slice of displayable values listed sequentially.
///
/// # Errors
/// If:
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
