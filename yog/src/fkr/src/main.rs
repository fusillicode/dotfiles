#![feature(exit_status_error)]

use fake::Fake;
use inquire::error::InquireResult;
use inquire::ui::RenderConfig;
use inquire::InquireError;
use inquire::Select;
use strum::IntoEnumIterator;
use strum_macros::Display;
use strum_macros::EnumIter;

fn main() -> anyhow::Result<()> {
    if let Some(selected_opt) = minimal_select(FkrOption::iter().collect()).cancellable_prompt()? {
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
            InquireError::OperationCanceled
            | InquireError::OperationInterrupted => Ok(None),
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

#[derive(EnumIter, Display)]
pub enum FkrOption {
    Uuidv4,
    Uuidv7,
    Email,
    UserAgent,
    IPv4,
    IPv6,
    MACAddress,
}

impl FkrOption {
    pub fn gen_string(&self) -> String {
        match self {
            FkrOption::Uuidv4 => fake::uuid::UUIDv4.fake::<String>(),
            FkrOption::Uuidv7 => fake::uuid::UUIDv7.fake::<String>(),
            FkrOption::Email => fake::faker::internet::en::SafeEmail().fake::<String>(),
            FkrOption::UserAgent => fake::faker::internet::en::UserAgent().fake::<String>(),
            FkrOption::MACAddress => fake::faker::internet::en::MACAddress().fake::<String>(),
            FkrOption::IPv4 => fake::faker::internet::en::IPv4().fake::<String>(),
            FkrOption::IPv6 => fake::faker::internet::en::IPv6().fake::<String>(),
        }
    }
}
