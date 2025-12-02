use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;

pub fn set_current(window: &Window) -> Option<()> {
    nvim_oxi::api::set_current_win(window)
        .inspect_err(|err| {
            crate::notify::error(format!("error setting current window | window={window:?}, err={err:?}"));
        })
        .ok()?;
    Some(())
}

pub fn get_buffer(win: &Window) -> Option<Buffer> {
    win.get_buf()
        .inspect_err(|err| {
            crate::notify::error(format!("error getting window buffer | window={win:?}, err={err:?}"));
        })
        .ok()
}
