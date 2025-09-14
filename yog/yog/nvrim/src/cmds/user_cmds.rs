use fkr::FkrOption;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::StringOrFunction;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateCommandOpts;
use nvim_oxi::api::opts::CreateCommandOptsBuilder;
use nvim_oxi::api::types::CommandArgs;
use strum::IntoEnumIterator;

const USER_CMDS: [(&str, &str); 6] = [
    ("CopyAll", ":%y+"),
    ("Highlights", ":FzfLua highlights"),
    ("LazyProfile", ":Lazy profile"),
    ("LazyUpdate", ":Lazy update"),
    ("SelectAll", "normal! ggVG"),
    ("Messages", ":Messages"),
];

/// Creates user commands and `Fkr*` snippet insertion commands.
pub fn create() {
    for (cmd_name, cmd) in USER_CMDS {
        create_user_cmd(cmd_name, cmd, &default_opts());
    }
    for fkr_opt in FkrOption::iter() {
        create_user_cmd(
            cmd_name(&fkr_opt),
            move |_| set_text_at_cursor_pos(&fkr_opt.gen_string()),
            &default_opts(),
        );
    }
}

/// Registers a single user command with Neovim.
fn create_user_cmd<Cmd>(name: &str, command: Cmd, opts: &CreateCommandOpts)
where
    Cmd: StringOrFunction<CommandArgs, ()>,
{
    if let Err(error) = nvim_oxi::api::create_user_command(name, command, opts) {
        crate::oxi_ext::api::notify_error(&format!(
            "cannot create user command {name} with opts {opts:#?}, error {error:#?}"
        ));
    }
}

/// Returns default options for user commands.
fn default_opts() -> CreateCommandOpts {
    CreateCommandOptsBuilder::default().build()
}

/// Inserts `text` at the current cursor position in the active buffer.
fn set_text_at_cursor_pos(text: &str) {
    let cur_win = Window::current();
    let Ok((row, col)) = cur_win.get_cursor().inspect_err(|error| {
        crate::oxi_ext::api::notify_error(&format!("cannot get cursor from window {cur_win:?}, error {error:?}"));
    }) else {
        return;
    };

    let row = row.saturating_sub(1);
    let line_range = row..row;
    let start_col = col;
    let end_col = col;
    let text = vec![text];

    let mut cur_buf = Buffer::current();
    if let Err(e) = cur_buf.set_text(line_range.clone(), start_col, end_col, text.clone()) {
        crate::oxi_ext::api::notify_error(&format!(
            "cannot set text {text:?} in buffer {cur_buf:?}, line_range {line_range:?}, start_col {start_col:?}, end_col {end_col:?}, error {e:?}"
        ));
    }
}

/// Returns the Nvim command name for a given [`FkrOption`].
const fn cmd_name(fkr_opt: &FkrOption) -> &str {
    match fkr_opt {
        FkrOption::Uuidv4 => "FkrUuidv4",
        FkrOption::Uuidv7 => "FkrUuidv7",
        FkrOption::Email => "FkrEmail",
        FkrOption::UserAgent => "FkrUserAgent",
        FkrOption::IPv4 => "FkrIPv4",
        FkrOption::IPv6 => "FkrIPv6",
        FkrOption::MACAddress => "FkrMACAddress",
    }
}
