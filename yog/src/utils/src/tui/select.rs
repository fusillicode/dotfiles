use inquire::InquireError;
use inquire::Select;

use crate::tui::minimal_render_config;
use crate::tui::ClosablePrompt;
use crate::tui::ClosablePromptError;

impl<'a, T: std::fmt::Display> ClosablePrompt<'a, T> for Select<'a, T> {
    fn closable_prompt(self) -> Result<T, ClosablePromptError> {
        self.prompt().map_or_else(
            |error| match error {
                InquireError::OperationCanceled | InquireError::OperationInterrupted => {
                    Err(ClosablePromptError::Closed)
                }
                InquireError::NotTTY
                | InquireError::InvalidConfiguration(_)
                | InquireError::IO(_)
                | InquireError::Custom(_) => Err(ClosablePromptError::Error(error)),
            },
            Result::Ok,
        )
    }
}

pub fn minimal<'a, T: std::fmt::Display>(options: Vec<T>) -> Select<'a, T> {
    Select::new("", options)
        .with_render_config(minimal_render_config())
        .without_help_message()
}
