use inquire::InquireError;
use inquire::MultiSelect;
use inquire::ui::RenderConfig;

pub fn minimal_multi_select<T: std::fmt::Display>(opts: Vec<T>) -> Result<Vec<T>, InquireError> {
    if opts.is_empty() {
        return Ok(opts);
    }
    closable_prompt(
        MultiSelect::new("", opts)
            .with_render_config(minimal_render_config())
            .without_help_message()
            .prompt(),
    )
}

fn closable_prompt<T: Default>(prompt_res: Result<T, InquireError>) -> Result<T, InquireError> {
    match prompt_res {
        Ok(res) => Ok(res),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(T::default()),
        Err(error) => Err(error),
    }
}

fn minimal_render_config<'a>() -> RenderConfig<'a> {
    RenderConfig::default_colored()
        .with_prompt_prefix("".into())
        .with_canceled_prompt_indicator("".into())
        .with_answered_prompt_prefix("".into())
}
