use inquire::error::InquireResult;
use inquire::Autocomplete;
use inquire::InquireError;
use inquire::Text;

use crate::tui::minimal_render_config;
use crate::tui::CancellablePrompt;

pub fn minimal<'a, T: std::fmt::Display>(ac: Option<Box<dyn Autocomplete>>) -> Text<'a> {
    let mut text = Text::new("").with_render_config(minimal_render_config());
    text.autocompleter = ac;
    text
}

impl<'a> CancellablePrompt<'a, String> for Text<'a> {
    fn cancellable_prompt(self) -> InquireResult<Option<String>> {
        self.prompt().map(Some).or_else(|e| match e {
            InquireError::OperationCanceled | InquireError::OperationInterrupted => Ok(None),
            InquireError::NotTTY
            | InquireError::InvalidConfiguration(_)
            | InquireError::IO(_)
            | InquireError::Custom(_) => Err(e),
        })
    }
}
