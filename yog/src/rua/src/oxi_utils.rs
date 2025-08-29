use nvim_oxi::api::types::LogLevel;

/// Notifies the user of an error message in Neovim.
pub fn notify_error(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Error, &Default::default()) {
        nvim_oxi::dbg!(format!("cannot notify error {msg:?}, error {error:#?}"));
    }
}

/// Notifies the user of a warning message in Neovim.
#[allow(dead_code)]
pub fn notify_warn(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Warn, &Default::default()) {
        nvim_oxi::dbg!(format!("cannot notify warning {msg:?}, error {error:#?}"));
    }
}
