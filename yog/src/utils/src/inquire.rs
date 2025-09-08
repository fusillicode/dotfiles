use inquire::InquireError;
use inquire::MultiSelect;
use inquire::ui::RenderConfig;

pub fn closable_prompt<T: Default>(prompt_res: Result<T, InquireError>) -> Result<T, InquireError> {
    match prompt_res {
        Ok(res) => Ok(res),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(T::default()),
        Err(error) => Err(error),
    }
}

pub fn minimal_multi_select<'a, T: std::fmt::Display>(options: Vec<T>) -> MultiSelect<'a, T> {
    MultiSelect::new("", options)
        .with_render_config(minimal_render_config())
        .without_help_message()
}

fn minimal_render_config<'a>() -> RenderConfig<'a> {
    RenderConfig::default_colored()
        .with_prompt_prefix("".into())
        .with_canceled_prompt_indicator("".into())
        .with_answered_prompt_prefix("".into())
}
