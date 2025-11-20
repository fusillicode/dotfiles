use core::fmt::Debug;

pub use nvim_oxi::api::opts;
pub use nvim_oxi::api::types;
use nvim_oxi::api::types::LogLevel;

use crate::dict;

/// Types that can be converted to a notification message for Nvim.
///
/// Implementors provide a way to transform themselves into a string suitable for display
/// in Nvim notifications.
///
/// # Returns
/// A string representation of the notifiable item.
pub trait Notifiable: Debug {
    fn to_msg(&self) -> impl AsRef<str>;
}

impl<T: Notifiable + ?Sized> Notifiable for &T {
    fn to_msg(&self) -> impl AsRef<str> {
        (*self).to_msg()
    }
}

impl Notifiable for color_eyre::Report {
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
    if let Err(err) = nvim_oxi::api::notify(notifiable.to_msg().as_ref(), LogLevel::Error, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify error | msg={notifiable:?} error={err:#?}"));
    }
}

/// Notifies the user of a warning message in Nvim.
pub fn warn<N: Notifiable>(notifiable: N) {
    if let Err(err) = nvim_oxi::api::notify(notifiable.to_msg().as_ref(), LogLevel::Warn, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify warning | msg={notifiable:?} error={err:#?}"));
    }
}
