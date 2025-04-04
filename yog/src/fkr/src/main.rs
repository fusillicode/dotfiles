#![feature(exit_status_error)]

use fkr::FkrOption;
use inquire::error::InquireResult;
use inquire::ui::RenderConfig;
use inquire::InquireError;
use inquire::Select;

fn main() -> anyhow::Result<()> {
    if let Some(selected_opt) = minimal_select(FkrOption::to_vec()).cancellable_prompt()? {
        println!("{}", selected_opt.gen_string())
    }

    Ok(())
}

trait SelectExt<'a, T: std::fmt::Display> {
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

fn minimal_select<'a, T: std::fmt::Display>(options: Vec<T>) -> Select<'a, T> {
    Select::new("", options)
        .with_render_config(minimal_render_config())
        .without_help_message()
}

fn minimal_render_config<'a>() -> RenderConfig<'a> {
    RenderConfig::default_colored()
        .with_prompt_prefix("".into())
        .with_canceled_prompt_indicator("".into())
        .with_answered_prompt_prefix("".into())
}
