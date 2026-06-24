//! Notification utilities for sending error and warning messages to Nvim.

use std::fmt::Debug;

pub use nvim_oxi::api::opts;
use nvim_oxi::api::opts::EchoOpts;
pub use nvim_oxi::api::types;

/// Types that can be converted to a notification message for Nvim.
///
/// Implementors provide a way to transform themselves into a string suitable for display
/// in Nvim notifications.
pub trait Notifiable: Debug {
    fn to_msg(&self) -> impl AsRef<str>;
}

impl<T: Notifiable + ?Sized> Notifiable for &T {
    fn to_msg(&self) -> impl AsRef<str> {
        (*self).to_msg()
    }
}

impl Notifiable for rootcause::Report {
    fn to_msg(&self) -> impl AsRef<str> {
        self.to_string()
    }
}

impl Notifiable for String {
    fn to_msg(&self) -> impl AsRef<str> {
        self
    }
}

impl Notifiable for &str {
    fn to_msg(&self) -> impl AsRef<str> {
        self
    }
}

/// Notifies the user of an error message in Nvim.
pub fn error<N: Notifiable>(notifiable: N) {
    if let Err(err) = echo(notifiable.to_msg().as_ref(), "ErrorMsg") {
        nvim_oxi::dbg!(format!("cannot notify error | msg={notifiable:?} error={err:#?}"));
    }
}

/// Notifies the user of a warning message in Nvim.
pub fn warn<N: Notifiable>(notifiable: N) {
    if let Err(err) = echo(notifiable.to_msg().as_ref(), "WarningMsg") {
        nvim_oxi::dbg!(format!("cannot notify warning | msg={notifiable:?} error={err:#?}"));
    }
}

fn echo(msg: &str, highlight: &str) -> Result<(), nvim_oxi::api::Error> {
    nvim_oxi::api::echo([(msg, Some(highlight))], true, &EchoOpts::default()).map(drop)
}
