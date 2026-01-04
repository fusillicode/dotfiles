//! Common utilities for Nvim API interactions, including variable setting and command execution.

use core::fmt::Debug;

use nvim_oxi::api::opts::CmdOpts;
use nvim_oxi::api::opts::ExecOpts;
use nvim_oxi::api::types::CmdInfosBuilder;
use nvim_oxi::conversion::ToObject;

/// Sets the value of a global Nvim variable `name` to `value`.
///
/// Wraps [`nvim_oxi::api::set_var`].
///
/// Errors are reported to Nvim via [`crate::notify::error`].
pub fn set_g_var<V: ToObject + Debug>(name: &str, value: V) {
    let msg = format!("error setting global var | name={name} value={value:#?}");
    if let Err(err) = nvim_oxi::api::set_var(name, value) {
        crate::notify::error(format!("{msg} | error={err:#?}"));
    }
}

/// Execute an ex command with optional arguments.
///
/// Wraps [`nvim_oxi::api::cmd`], reporting failures through [`crate::notify::error`].
///
/// # Arguments
/// - `cmd` The ex command to execute.
/// - `args` Optional list of arguments for the command.
///
/// # Errors
/// Errors from [`nvim_oxi::api::cmd`] are propagated after logging via [`crate::notify::error`].
pub fn exec_vim_cmd(
    cmd: impl AsRef<str> + Debug + std::marker::Copy,
    args: Option<&[impl AsRef<str> + Debug]>,
) -> Result<Option<String>, nvim_oxi::api::Error> {
    let mut cmd_infos_builder = CmdInfosBuilder::default();
    cmd_infos_builder.cmd(cmd.as_ref());

    if let Some(args) = args {
        cmd_infos_builder.args(args.iter().map(|s| s.as_ref().to_string()));
    }
    nvim_oxi::api::cmd(&cmd_infos_builder.build(), &CmdOpts::default()).inspect_err(|err| {
        crate::notify::error(format!(
            "error executing cmd | cmd={cmd:?} args={args:#?} error={err:#?}",
        ));
    })
}

/// Executes Vimscript source code and returns the output if any.
///
/// # Arguments
/// - `src` The Vimscript source code to execute.
/// - `opts` Optional execution options; defaults to [`ExecOpts::default`] if [`None`].
///
/// # Errors
/// Errors are reported to Nvim via [`crate::notify::error`].
#[allow(clippy::option_option)]
pub fn exec_vim_script(src: &str, opts: Option<ExecOpts>) -> Option<Option<String>> {
    let opts = opts.unwrap_or_default();
    Some(
        nvim_oxi::api::exec2(src, &opts)
            .inspect_err(|err| {
                crate::notify::error(format!(
                    "error executing Vimscript | src={src:?} opts={opts:?} error={err:?}"
                ));
            })
            .ok()?
            .map(|s| s.to_string()),
    )
}
