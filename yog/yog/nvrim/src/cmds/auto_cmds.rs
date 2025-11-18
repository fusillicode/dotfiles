//! Autocommand group and definition helpers.
//!
//! Creates yank highlight, autosave, and quickfix configuration autocmds with resilient error
//! reporting (failures logged, rest continue). Provides granular `create_autocmd` utility.

use core::fmt::Debug;
use core::marker::Copy;

use nvim_oxi::api::opts::CreateAugroupOptsBuilder;
use nvim_oxi::api::opts::CreateAutocmdOptsBuilder;
use nvim_oxi::api::opts::SetKeymapOptsBuilder;
use nvim_oxi::api::types::Mode;

/// Creates Nvim autocommands and their augroups.
///
/// Includes yank highlight, autosave on focus loss / buffer leave, and quickfix
/// specific key mappings & configuration.
pub fn create() {
    create_autocmd(
        ["TextYankPost"],
        "YankHighlight",
        CreateAutocmdOptsBuilder::default().command(":lua vim.highlight.on_yank()"),
    );

    create_autocmd(
        ["BufLeave", "FocusLost"],
        "AutosaveBuffers",
        CreateAutocmdOptsBuilder::default().command(":silent! wa!"),
    );

    create_autocmd(
        ["FileType"],
        "QuickfixConfig",
        CreateAutocmdOptsBuilder::default().patterns(["qf"]).callback(|_| {
            let opts = SetKeymapOptsBuilder::default().noremap(true).build();

            crate::keymaps::set(&[Mode::Normal], "<c-n>", ":cn<cr>", &opts);
            crate::keymaps::set(&[Mode::Normal], "<c-p>", ":cp<cr>", &opts);
            crate::keymaps::set(&[Mode::Normal], "<c-x>", ":ccl<cr>", &opts);
            let _ = ytil_nvim_oxi::api::exec_vim_cmd("resize", Some(&["7"]));

            true
        }),
    );

    crate::scrolloff::create_autocmd();
}

/// Creates an autocommand group and associated autocommands for `events`.
///
/// Errors are reported to Nvim (and swallowed) so that one failing definition
/// does not abort the rest of the setup.
pub fn create_autocmd<'a, I>(events: I, augroup_name: &str, opts_builder: &mut CreateAutocmdOptsBuilder)
where
    I: IntoIterator<Item = &'a str> + Debug + Copy,
{
    if let Err(err) =
        nvim_oxi::api::create_augroup(augroup_name, &CreateAugroupOptsBuilder::default().clear(true).build())
            .inspect_err(|err| {
                ytil_nvim_oxi::api::notify_error(format!(
                    "error creating augroup | name={augroup_name:?} error={err:#?}"
                ));
            })
            .and_then(|group| nvim_oxi::api::create_autocmd(events, &opts_builder.group(group).build()))
    {
        ytil_nvim_oxi::api::notify_error(format!(
            "error creating auto command | augroup={augroup_name:?} events={events:#?} error={err:#?}"
        ));
    }
}
