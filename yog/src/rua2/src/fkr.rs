use fkr::FkrOption;
use nvim_oxi::Function;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use nvim_oxi::api::opts::CreateCommandOptsBuilder;
use strum::IntoEnumIterator;

pub fn create_cmds() -> Object {
    Object::from(Function::<(), nvim_oxi::Result<_>>::from_fn(create_cmds_core))
}

fn create_cmds_core(_: ()) -> nvim_oxi::Result<()> {
    for fkr_opt in FkrOption::iter() {
        nvim_oxi::api::create_user_command(
            cmd_name(&fkr_opt),
            move |_| -> nvim_oxi::Result<()> {
                let mut cur_buf = Buffer::current();
                let (row, col) = Window::current().get_cursor()?;
                cur_buf.set_text(row..row + 1, col, col, vec![fkr_opt.gen_string()])?;

                Ok(())
            },
            &CreateCommandOptsBuilder::default().build(),
        )?;
    }
    Ok(())
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
