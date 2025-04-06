#![feature(exit_status_error)]

use fkr::FkrOption;
use inquire::error::InquireResult;
use inquire::ui::RenderConfig;
use inquire::InquireError;
use inquire::Select;

/// Prints on terminal a fake value generated on the fly based on the selection.
/// If "cp" is supplied as first argument also copies the generated value to the system clipboard.
/// If the selection is cancelled (<ESC>) or interrupted (<CTRL-C>) exists with success without
/// printing anything.
fn main() -> anyhow::Result<()> {
    if let Some(generated_value) = minimal_select(FkrOption::to_vec())
        .cancellable_prompt()?
        .map(|fkr_opt| fkr_opt.gen_string())
    {
        println!("{generated_value}");
        if utils::system::get_args()
            .first()
            .is_some_and(|arg| arg == "cp")
        {
            utils::system::copy_to_system_clipboard(&mut generated_value.as_bytes())?;
        }
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
