//! Vim option helpers and bulk setters exposed to Lua.
//!
//! Provides a dictionary `vim_opts.dict()` with batch application (`set_all`) and granular option mutation
//! utilities wrapping [`nvim_oxi::api::set_option_value`], emitting notifications via
//! Uses `ytil_nvim_oxi::api::notify_error` on failure.

use core::fmt::Debug;
use core::marker::Copy;
use std::fmt::Write as _;

use nvim_oxi::Dictionary;
use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::api::opts::OptionOptsBuilder;
use nvim_oxi::api::opts::OptionScope;
use nvim_oxi::conversion::ToObject;

/// [`Dictionary`] of `vim.opts` helpers.
pub fn dict() -> Dictionary {
    dict! {
        "set_all": fn_from!(set_all),
    }
}

/// Sets a Vim option by `name` to `value` within the given [`OptionOpts`].
///
/// Errors are notified to Nvim via `ytil_nvim_oxi::api::notify_error`.
pub fn set<Opt: ToObject + Debug + Copy>(name: &str, value: Opt, opts: &OptionOpts) {
    if let Err(error) = nvim_oxi::api::set_option_value(name, value, opts) {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot set option | name={name:?} value={value:#?} opts={opts:#?} error={error:#?}"
        ));
    }
}

/// Appends to a var named `name` the supplied value `value`.
///
/// The current value is read as a [`String`] and modified by appending the supplied one with a
/// comma.
///
/// Errors are notified to Nvim via `ytil_nvim_oxi::api::notify_error`.
pub fn append(name: &str, value: &str, opts: &OptionOpts) {
    let Ok(mut cur_value) = nvim_oxi::api::get_option_value::<String>(name, opts).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot get option current value | name={name:?} opts={opts:#?} value_to_append={value:#?} error={error:#?}"
        ));
    }) else {
        return;
    };
    // This shenanigan with `comma` and `write!` is to avoid additional allocations
    let comma = if cur_value.is_empty() { "" } else { "," };
    if let Err(error) = write!(cur_value, "{comma}{value}") {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot append option value | name={name:?} cur_value={cur_value} append_value={value} opts={opts:#?} error={error:#?}"
        ));
    }
    set(name, &*cur_value, opts);
}

/// Returns [`OptionOpts`] configured for the global scope.
pub fn global_scope() -> OptionOpts {
    OptionOptsBuilder::default().scope(OptionScope::Global).build()
}

/// Sets the desired Nvim options.
fn set_all(_: ()) {
    let global_scope = global_scope();

    set("autoindent", true, &global_scope);
    set("backspace", "indent,eol,start", &global_scope);
    set("breakindent", true, &global_scope);
    set("completeopt", "menuone,noselect", &global_scope);
    set("cursorline", true, &global_scope);
    set("expandtab", true, &global_scope);
    set("hlsearch", true, &global_scope);
    set("ignorecase", true, &global_scope);
    set("laststatus", 3, &global_scope);
    set("list", true, &global_scope);
    set("number", true, &global_scope);
    set("shiftwidth", 2, &global_scope);
    set("shortmess", "ascIF", &global_scope);
    set("showmode", false, &global_scope);
    set("showtabline", 0, &global_scope);
    set("sidescroll", 1, &global_scope);
    set("signcolumn", "no", &global_scope);
    set("smartcase", true, &global_scope);
    set("splitbelow", true, &global_scope);
    set("splitright", true, &global_scope);
    set(
        "statuscolumn",
        r#"%{%v:lua.require("statuscolumn").draw(v:lnum)%}"#,
        &global_scope,
    );
    set(
        "statusline",
        r#"%{%v:lua.require("statusline").draw()%}"#,
        &global_scope,
    );
    set("swapfile", false, &global_scope);
    set("tabstop", 2, &global_scope);
    set("undofile", true, &global_scope);
    set("updatetime", 250, &global_scope);
    set("wrap", true, &global_scope);

    append("clipboard", "unnamedplus", &global_scope);
    append("iskeyword", "-", &global_scope);
    append("jumpoptions", "stack", &global_scope);
}
