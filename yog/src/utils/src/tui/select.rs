use color_eyre::eyre::eyre;
use inquire::InquireError;
use inquire::Select;

use crate::tui::minimal_render_config;
use crate::tui::ClosablePrompt;
use crate::tui::ClosablePromptError;

pub fn minimal<'a, T: std::fmt::Display>(options: Vec<T>) -> Select<'a, T> {
    Select::new("", options)
        .with_render_config(minimal_render_config())
        .without_help_message()
}

/// Get the entry that matches the supplied CLI argument or the one selected via the presented
/// TUI if no CLI argument is supplied.
///
/// # Parameters
/// - `args`: all the CLI arguments
/// - `entries`: list of available entries
///
/// # Returns
/// - `Ok(Some(entry))` if an entry is found by CLI argument or TUI selection
/// - `Ok(None)` if the user closes the TUI selection
/// - `Err` if no entry if found by CLI argument or if TUI lookup fails
pub fn get_entry_from_first_arg_or_tui<'a, T, F, P>(
    args: &'a [String],
    entries: Vec<T>,
    finder_by_arg: F,
) -> color_eyre::Result<Option<T>>
where
    T: Clone + std::fmt::Debug + std::fmt::Display,
    F: Fn(&'a str) -> P,
    P: FnMut(&T) -> bool + 'a,
{
    if let Some(arg) = args.first() {
        let mut finder = finder_by_arg(arg);
        return Ok(Some(
            entries
                .iter()
                .find(|x| finder(*x))
                .cloned()
                .ok_or_else(|| {
                    eyre!("no entry found matching the supplied arg {arg} in entries {entries:?}")
                })?,
        ));
    }
    match minimal::<T>(entries).closable_prompt() {
        Ok(pgpass_entry) => Ok(Some(pgpass_entry)),
        Err(ClosablePromptError::Closed) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

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
