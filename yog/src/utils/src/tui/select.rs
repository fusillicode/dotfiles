use inquire::error::InquireResult;
use inquire::InquireError;
use inquire::Select;

use crate::tui::minimal_render_config;

pub trait SelectExt<'a, T: std::fmt::Display> {
    fn cancellable_prompt(self) -> InquireResult<Option<T>>;
}

impl<'a, T: std::fmt::Display> SelectExt<'a, T> for Select<'a, T> {
    fn cancellable_prompt(self) -> InquireResult<Option<T>> {
        self.prompt().map(Some).or_else(|e| match e {
            InquireError::OperationCanceled | InquireError::OperationInterrupted => Ok(None),
            InquireError::NotTTY
            | InquireError::InvalidConfiguration(_)
            | InquireError::IO(_)
            | InquireError::Custom(_) => Err(e),
        })
    }
}

pub fn minimal<'a, T: std::fmt::Display>(options: Vec<T>) -> Select<'a, T> {
    Select::new("", options)
        .with_render_config(minimal_render_config())
        .without_help_message()
}
