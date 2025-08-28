use nvim_oxi::api::types::LogLevel;

pub fn notify_error(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Error, &Default::default()) {
        nvim_oxi::dbg!(format!("can't notify error {msg:?}, error {error:#?}"));
    }
}

#[allow(dead_code)]
pub fn notify_warn(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Warn, &Default::default()) {
        nvim_oxi::dbg!(format!("can't notify warning {msg:?}, error {error:#?}"));
    }
}
