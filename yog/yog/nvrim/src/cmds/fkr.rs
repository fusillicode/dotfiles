use fkr::FkrOption;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use strum::IntoEnumIterator;

/// Creates Nvim user commands for generating fake data using FKR.
pub fn create_all(_: ()) {
    for fkr_opt in FkrOption::iter() {
        crate::cmds::create_user_cmd(
            cmd_name(&fkr_opt),
            move |_| set_text_at_cursor_pos(&fkr_opt.gen_string()),
            &crate::cmds::default_opts(),
        );
    }
}

fn set_text_at_cursor_pos(text: &str) {
    let cur_win = Window::current();
    let Ok((row, col)) = cur_win.get_cursor().inspect_err(|error| {
        crate::oxi_ext::notify_error(&format!("cannot get cursor from window {cur_win:?}, error {error:?}"));
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
        crate::oxi_ext::notify_error(&format!(
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
