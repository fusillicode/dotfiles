use inquire::ui::RenderConfig;

pub mod git_branches_autocomplete;
pub mod select;
pub mod text;

pub use inquire;
use inquire::InquireError;
use thiserror::Error;

pub trait ClosablePrompt<'a, T: std::fmt::Display> {
    fn closable_prompt(self) -> Result<T, ClosablePromptError>;
}

#[derive(Error, Debug)]
pub enum ClosablePromptError {
    #[error("prompt has been closed, i.e. cancelled (<ESC>) or interrupted (<CTRL-C>) by user")]
    Closed,
    #[error(transparent)]
    Error(InquireError),
}

fn minimal_render_config<'a>() -> RenderConfig<'a> {
    RenderConfig::default_colored()
        .with_prompt_prefix("".into())
        .with_canceled_prompt_indicator("".into())
        .with_answered_prompt_prefix("".into())
}
