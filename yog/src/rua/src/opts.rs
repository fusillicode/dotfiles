use nvim_oxi::api::opts::OptionOpts;
use nvim_oxi::api::opts::OptionOptsBuilder;
use nvim_oxi::api::opts::OptionScope;
use nvim_oxi::conversion::ToObject;

pub fn set(_: ()) {
    let opts = OptionOptsBuilder::default().scope(OptionScope::Global).build();

    set_opt("autoindent", true, &opts);
    set_opt("backspace", "indent,eol,start", &opts);
    set_opt("breakindent", true, &opts);
    set_opt("completeopt", "menuone,noselect", &opts);
    set_opt("cursorline", true, &opts);
    set_opt("expandtab", true, &opts);
    set_opt("hlsearch", true, &opts);
    set_opt("ignorecase", true, &opts);
    set_opt("laststatus", 3, &opts);
    set_opt("list", true, &opts);
    set_opt("mouse", "a", &opts);
    set_opt("number", true, &opts);
    set_opt("shiftwidth", 2, &opts);
    set_opt("shortmess", "ascIF", &opts);
    set_opt("showmode", false, &opts);
    set_opt("showtabline", 0, &opts);
    set_opt("sidescroll", 1, &opts);
    set_opt("signcolumn", "no", &opts);
    set_opt("smartcase", true, &opts);
    set_opt("splitbelow", true, &opts);
    set_opt("splitright", true, &opts);
    set_opt(
        "statuscolumn",
        r#"%{%v:lua.require("statuscolumn").draw(v:lnum)%}"#,
        &opts,
    );
    set_opt("statusline", r#"%{%v:lua.require("statusline").draw()%}"#, &opts);
    set_opt("swapfile", false, &opts);
    set_opt("tabstop", 2, &opts);
    set_opt("undofile", true, &opts);
    set_opt("updatetime", 250, &opts);
    set_opt("wrap", false, &opts);

    append_to_opt("clipboard", "unnamedplus", &opts);
    append_to_opt("iskeyword", "-", &opts);
    append_to_opt("jumpoptions", "stack", &opts);
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
