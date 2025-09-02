use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::api::opts::OptionOptsBuilder;
use nvim_oxi::api::opts::OptionScope;
use nvim_oxi::conversion::ToObject;

pub fn set(_: ()) {
    let global_scope = global_scope();

    set_opt("autoindent", true, &global_scope);
    set_opt("backspace", "indent,eol,start", &global_scope);
    set_opt("breakindent", true, &global_scope);
    set_opt("completeopt", "menuone,noselect", &global_scope);
    set_opt("cursorline", true, &global_scope);
    set_opt("expandtab", true, &global_scope);
    set_opt("hlsearch", true, &global_scope);
    set_opt("ignorecase", true, &global_scope);
    set_opt("laststatus", 3, &global_scope);
    set_opt("list", true, &global_scope);
    set_opt("mouse", "a", &global_scope);
    set_opt("number", true, &global_scope);
    set_opt("shiftwidth", 2, &global_scope);
    set_opt("shortmess", "ascIF", &global_scope);
    set_opt("showmode", false, &global_scope);
    set_opt("showtabline", 0, &global_scope);
    set_opt("sidescroll", 1, &global_scope);
    set_opt("signcolumn", "no", &global_scope);
    set_opt("smartcase", true, &global_scope);
    set_opt("splitbelow", true, &global_scope);
    set_opt("splitright", true, &global_scope);
    set_opt(
        "statuscolumn",
        r#"%{%v:lua.require("statuscolumn").draw(v:lnum)%}"#,
        &global_scope,
    );
    set_opt(
        "statusline",
        r#"%{%v:lua.require("statusline").draw()%}"#,
        &global_scope,
    );
    set_opt("swapfile", false, &global_scope);
    set_opt("tabstop", 2, &global_scope);
    set_opt("undofile", true, &global_scope);
    set_opt("updatetime", 250, &global_scope);
    set_opt("wrap", false, &global_scope);

    append_to_opt("clipboard", "unnamedplus", &global_scope);
    append_to_opt("iskeyword", "-", &global_scope);
    append_to_opt("jumpoptions", "stack", &global_scope);
}

pub fn set_opt<Opt: ToObject + core::fmt::Debug + core::marker::Copy>(name: &str, value: Opt, opts: &OptionOpts) {
    if let Err(error) = nvim_oxi::api::set_option_value(name, value, opts) {
        crate::oxi_ext::notify_error(&format!(
            "cannot set opt {name:?} value {value:#?} with {opts:#?}, error {error:#?}"
        ));
    }
}

pub fn append_to_opt(name: &str, value: &str, opts: &OptionOpts) {
    let Ok(mut cur_value) = nvim_oxi::api::get_option_value::<String>(name, opts).inspect_err(|error| {
        crate::oxi_ext::notify_error(&format!(
            "cannot get current value of opt {name:?} with {opts:#?} to append {value:#?} , error {error:#?}"
        ));
    }) else {
        return;
    };
    cur_value.push_str(&format!(",{value}"));
    set_opt(name, value, opts);
}

pub fn global_scope() -> OptionOpts {
    OptionOptsBuilder::default().scope(OptionScope::Global).build()
}
