use nvim_oxi::api::opts::SetKeymapOpts;
use nvim_oxi::api::opts::SetKeymapOptsBuilder;
use nvim_oxi::api::types::Mode;

pub fn set_all(_: ()) {
    let empty_opts = SetKeymapOptsBuilder::default().build();

    set(&[Mode::Terminal], "<Esc>", "<c-\\><c-n>", &empty_opts);
    set(&[Mode::Insert], "<c-a>", "<esc>^i", &empty_opts);
    set(&[Mode::Normal], "<c-a>", "^i", &empty_opts);
    set(&[Mode::Insert], "<c-e>", "<end>", &empty_opts);
    set(&[Mode::Normal], "<c-e>", "$a", &empty_opts);

    set(&[Mode::NormalVisualOperator], "gn", ":bn<cr>", &empty_opts);
    set(&[Mode::NormalVisualOperator], "gp", ":bp<cr>", &empty_opts);
    set(&[Mode::Normal, Mode::Visual], "gh", "0", &empty_opts);
    set(&[Mode::Normal, Mode::Visual], "gl", "$", &empty_opts);
    set(&[Mode::Normal, Mode::Visual], "gs", "_", &empty_opts);

    // -- https://stackoverflow.com/a/3003636
    // keymap_set('n', 'i', function()
    //   return (vim.fn.empty(vim.fn.getline('.')) == 1 and '\"_cc' or 'i')
    // end, { expr = true, })

    set(&[Mode::Insert], "<c-a>", "<esc>^i", &empty_opts);
    set(&[Mode::Normal], "<c-a>", "^i", &empty_opts);
    set(&[Mode::Insert], "<c-e>", "<end>", &empty_opts);
    set(&[Mode::Normal], "<c-e>", "$a", &empty_opts);

    // -- https://github.com/Abstract-IDE/abstract-autocmds/blob/main/lua/abstract-autocmds/mappings.lua#L8-L14
    // keymap_set('n', 'dd', function()
    //   return (vim.api.nvim_get_current_line():match('^%s*$') and '"_dd' or 'dd')
    // end, { noremap = true, expr = true, })

    set(&[Mode::Normal, Mode::Visual], "x", r#""_x"#, &empty_opts);
    set(&[Mode::Normal, Mode::Visual], "X", r#""_X"#, &empty_opts);

    set(
        &[Mode::Normal, Mode::Visual],
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
    set(&[Mode::Normal, Mode::Visual], "U", "<c-r>", &empty_opts);

    set(
        &[Mode::Normal, Mode::Visual],
        "<leader><leader>",
        ":silent :w!<cr>",
        &empty_opts,
    );
    set(&[Mode::Normal, Mode::Visual], "<leader>x", ":bd<cr>", &empty_opts);
    set(&[Mode::Normal, Mode::Visual], "<leader>X", ":bd!<cr>", &empty_opts);
    set(&[Mode::Normal, Mode::Visual], "<leader>q", ":q<cr>", &empty_opts);
    set(&[Mode::Normal, Mode::Visual], "<leader>Q", ":q!<cr>", &empty_opts);

    set(&[Mode::Normal, Mode::Visual], "<c-;>", ":set wrap!<cr>", &empty_opts);
    set(&[Mode::Normal], "<esc>", r#":noh<cr>:echo""<cr>"#, &empty_opts);

    // function M.visual_esc()
    //   return ":<c-u>'" .. (vim.fn.line('.') < vim.fn.line('v') and '<' or '>') .. '<cr>' .. M.normal_esc
    // end
    // set_keymap(
    //     &[Mode::Visual],
    //     "<esc>",
    //     "require('utils').visual_esc",
    //     &empty_opts().expr(true).build(),
    // )
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
