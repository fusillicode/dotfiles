use inquire::Autocomplete;
use inquire::InquireError;
use inquire::Text;

use crate::tui::ClosablePrompt;
use crate::tui::ClosablePromptError;
use crate::tui::minimal_render_config;

pub fn minimal<'a, T: std::fmt::Display>(ac: Option<Box<dyn Autocomplete>>) -> Text<'a> {
    let mut text = Text::new("")
        .with_render_config(minimal_render_config())
        .with_help_message("");
    text.autocompleter = ac;
    text
}

impl<'a> ClosablePrompt<'a, String> for Text<'a> {
    fn closable_prompt(self) -> Result<String, ClosablePromptError> {
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
