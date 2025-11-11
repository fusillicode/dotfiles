use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateAutocmdOptsBuilder;
use nvim_oxi::api::types::AutocmdCallbackArgs;

pub fn create_autocmd() {
    crate::cmds::create_autocmd(
        ["BufEnter", "WinEnter", "WinNew", "VimResized"],
        "ScrolloffFraction",
        CreateAutocmdOptsBuilder::default().patterns(["*"]).callback(callback),
    );
}

fn callback(_: AutocmdCallbackArgs) -> bool {
    let Ok(height) = Window::current().get_height().map(f64::from).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get nvim window height | error={error:?}"));
    }) else {
        return false;
    };
    let scrolloff = (height * 0.5).floor() as i64;
    crate::vim_opts::set("scrolloff", scrolloff, &crate::vim_opts::global_scope());
    false
}
