use nvim_oxi::api::opts::SetKeymapOpts;
use nvim_oxi::api::opts::SetKeymapOptsBuilder;
use nvim_oxi::api::types::Mode;

const NV_MODE: [Mode; 2] = [Mode::Normal, Mode::Visual];
const NVOP_MODE: [Mode; 1] = [Mode::NormalVisualOperator];
pub const NORMAL_ESC: &str = r#":noh<cr>:echo""<cr>"#;

pub fn set_all(_: ()) {
    let empty_opts = SetKeymapOptsBuilder::default().build();

    set(&[Mode::Terminal], "<Esc>", "<c-\\><c-n>", &empty_opts);
    set(&[Mode::Insert], "<c-a>", "<esc>^i", &empty_opts);
    set(&[Mode::Normal], "<c-a>", "^i", &empty_opts);
    set(&[Mode::Insert], "<c-e>", "<end>", &empty_opts);
    set(&[Mode::Normal], "<c-e>", "$a", &empty_opts);

    set(&NVOP_MODE, "gn", ":bn<cr>", &empty_opts);
    set(&NVOP_MODE, "gp", ":bp<cr>", &empty_opts);
    set(&NV_MODE, "gh", "0", &empty_opts);
    set(&NV_MODE, "gl", "$", &empty_opts);
    set(&NV_MODE, "gs", "_", &empty_opts);

    set(&NV_MODE, "x", r#""_x"#, &empty_opts);
    set(&NV_MODE, "X", r#""_X"#, &empty_opts);

    set(
        &NV_MODE,
        "<leader>yf",
        r#":let @+ = expand("%") . ":" . line(".")<cr>"#,
        &empty_opts,
    );
    set(&[Mode::Visual], "y", "ygv<esc>", &empty_opts);
    set(&[Mode::Visual], "p", r#""_dP"#, &empty_opts);

    set(&[Mode::Visual], ">", ">gv", &empty_opts);
    set(&[Mode::Visual], "<", "<gv", &empty_opts);
    set(&[Mode::Normal], ">", ">>", &empty_opts);
    set(&[Mode::Normal], "<", "<<", &empty_opts);
    set(&NV_MODE, "U", "<c-r>", &empty_opts);

    set(&NV_MODE, "<leader><leader>", ":silent :w!<cr>", &empty_opts);
    set(&NV_MODE, "<leader>x", ":bd<cr>", &empty_opts);
    set(&NV_MODE, "<leader>X", ":bd!<cr>", &empty_opts);
    set(&NV_MODE, "<leader>q", ":q<cr>", &empty_opts);
    set(&NV_MODE, "<leader>Q", ":q!<cr>", &empty_opts);

    set(&NV_MODE, "<c-;>", ":set wrap!<cr>", &empty_opts);
    set(&[Mode::Normal], "<esc>", r#":noh<cr>:echo""<cr>"#, &empty_opts);
}

// Vim: Smart indent when entering insert mode on blank line?
pub fn smart_ident_on_blank_line(_: ()) -> String {
    apply_on_current_line_or_unwrap(|line| if line.is_empty() { r#""_cc"# } else { "i" }, "i")
}

// Smart deletion, dd
// It solves the issue, where you want to delete empty line, but dd will override your last yank.
// Code below will check if you are deleting empty line, if so - use black hole register.
// [src: https://www.reddit.com/r/neovim/comments/w0jzzv/comment/igfjx5y/?utm_source=share&utm_medium=web2x&context=3]
pub fn smart_dd_no_yank_empty_line(_: ()) -> String {
    apply_on_current_line_or_unwrap(
        |line| {
            if line.chars().all(char::is_whitespace) {
                r#""_dd"#
            } else {
                "dd"
            }
        },
        "dd",
    )
}

pub fn visual_esc(_: ()) -> String {
    let current_line: i64 = nvim_oxi::api::call_function("line", (".",))
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get current line, error {error:#?}"));
        })
        .unwrap_or(0);
    let visual_line: i64 = nvim_oxi::api::call_function("line", ("v",))
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get visual line, error {error:#?}"));
        })
        .unwrap_or(0);
    format!(
        ":<c-u>'{}{NORMAL_ESC}",
        if current_line < visual_line { "<" } else { ">" }
    )
}

pub fn set(modes: &[Mode], lhs: &str, rhs: &str, opts: &SetKeymapOpts) {
    for mode in modes {
        if let Err(error) = nvim_oxi::api::set_keymap(*mode, lhs, rhs, opts) {
            crate::oxi_ext::notify_error(&format!(
                "cannot set keymap with mode {mode:#?}, lhs {lhs}, rhs {rhs} and opts {opts:#?}, error {error:#?}"
            ));
        }
    }
}

fn apply_on_current_line_or_unwrap<'a, F: FnOnce(String) -> &'a str>(fun: F, default: &'a str) -> String {
    nvim_oxi::api::get_current_line()
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get current line, error {error:#?}"));
        })
        .map(fun)
        .unwrap_or(default)
        .to_string()
}
