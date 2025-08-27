use fkr::FkrOption;
use nvim_oxi::Function;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateCommandOptsBuilder;
use strum::IntoEnumIterator;

pub fn create_cmds() -> Object {
    Object::from(Function::<(), _>::from_fn(create_cmds_core))
}

fn create_cmds_core(_: ()) {
    for fkr_opt in FkrOption::iter() {
        if let Err(error) = nvim_oxi::api::create_user_command(
            cmd_name(&fkr_opt),
            move |_| {
                let cur_win = Window::current();
                let Ok((row, col)) = cur_win.get_cursor().inspect_err(|error| {
                    crate::oxi_utils::notify_error(&format!("can't get cursor from window {cur_win:?}, error {error:?}"))
                }) else {
                    return;
                };

                let row = row.saturating_sub(1);
                let line_range = row..row;
                let start_col = col;
                let end_col = col;
                let text = vec![fkr_opt.gen_string()];

                let mut cur_buf = Buffer::current();
                if let Err(e) = cur_buf.set_text(line_range.clone(), start_col, end_col, text.clone()) {
                    crate::oxi_utils::notify_error(&format!(
                        "can't set text {text:?} in buffer {cur_buf:?}, line_range {line_range:?}, start_col {start_col:?}, end_col {end_col:?}, error {e:?}"
                    ))
                }
            },
            &CreateCommandOptsBuilder::default().build(),
        ) {
            crate::oxi_utils::notify_error(&format!("can't create user cmd, error {error:#?}"));
        };
    }
}

fn cmd_name(fkr_opt: &FkrOption) -> &str {
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
