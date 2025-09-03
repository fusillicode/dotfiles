use nvim_oxi::api::StringOrFunction;
use nvim_oxi::api::opts::CreateAugroupOptsBuilder;
use nvim_oxi::api::opts::CreateAutocmdOptsBuilder;
use nvim_oxi::api::opts::CreateCommandOpts;
use nvim_oxi::api::opts::CreateCommandOptsBuilder;
use nvim_oxi::api::opts::SetKeymapOpts;
use nvim_oxi::api::opts::SetKeymapOptsBuilder;
use nvim_oxi::api::types::CommandArgs;
use nvim_oxi::api::types::Mode;

pub mod fkr;

const USER_CMDS: [(&str, &str); 6] = [
    ("CopyAll", ":%y+"),
    ("Highlights", ":FzfLua highlights"),
    ("LazyProfile", ":Lazy profile"),
    ("LazyUpdate", ":Lazy update"),
    ("SelectAll", "normal! ggVG"),
    ("Messages", ":Messages"),
];

pub fn create(_: ()) {
    fkr::create_all(());

    for (cmd_name, cmd) in USER_CMDS {
        create_user_cmd(cmd_name, cmd, &default_opts());
    }

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

            set_keymap(Mode::Normal, "<c-n>", ":cn<cr>", &opts);
            set_keymap(Mode::Normal, "<c-p>", ":cp<cr>", &opts);
            set_keymap(Mode::Normal, "<c-x>", ":ccl<cr>", &opts);

            true
        }),
    );
}

fn create_user_cmd<Cmd>(name: &str, command: Cmd, opts: &CreateCommandOpts)
where
    Cmd: StringOrFunction<CommandArgs, ()>,
{
    if let Err(error) = nvim_oxi::api::create_user_command(name, command, opts) {
        crate::oxi_ext::notify_error(&format!(
            "cannot create user command {name} with opts {opts:#?}, error {error:#?}"
        ));
    }
}

fn create_autocmd<'a, I>(events: I, augroup_name: &str, opts_builder: &mut CreateAutocmdOptsBuilder)
where
    I: IntoIterator<Item = &'a str> + core::fmt::Debug + core::marker::Copy,
{
    if let Err(error) =
        nvim_oxi::api::create_augroup(augroup_name, &CreateAugroupOptsBuilder::default().clear(true).build())
            .inspect_err(|error| {
                crate::oxi_ext::notify_error(&format!(
                    "cannot create augroup with name {augroup_name:#?}, error {error:#?}"
                ));
            })
            .and_then(|group| nvim_oxi::api::create_autocmd(events, &opts_builder.group(group).build()))
    {
        crate::oxi_ext::notify_error(&format!(
            "cannot create auto command for events {events:#?} and augroup {augroup_name}, error {error:#?}"
        ));
    }
}

fn set_keymap(mode: Mode, lhs: &str, rhs: &str, opts: &SetKeymapOpts) {
    if let Err(error) = nvim_oxi::api::set_keymap(mode, lhs, rhs, opts) {
        crate::oxi_ext::notify_error(&format!(
            "cannot set keymap with mode {mode:#?}, lhs {lhs}, rhs {rhs} and opts {opts:#?}, error {error:#?}"
        ));
    }
}

pub fn default_opts() -> CreateCommandOpts {
    CreateCommandOptsBuilder::default().build()
}
