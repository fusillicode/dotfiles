use inquire::ui::RenderConfig;

pub mod git_branches_autocomplete;
pub mod select;
pub mod text;

pub use inquire;

fn minimal_render_config<'a>() -> RenderConfig<'a> {
    RenderConfig::default_colored()
        .with_prompt_prefix("".into())
        .with_canceled_prompt_indicator("".into())
        .with_answered_prompt_prefix("".into())
}
