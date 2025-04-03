#![feature(exit_status_error)]

use fake::Fake;
use inquire::ui::RenderConfig;
use inquire::Select;
use strum::IntoEnumIterator;
use strum_macros::Display;
use strum_macros::EnumIter;

fn main() -> anyhow::Result<()> {
    let selection_res = minimal_select(Dummy::iter().collect())
        .prompt()
        .map(Some)
        .or_else(|e| match e {
            inquire::InquireError::OperationCanceled
            | inquire::InquireError::OperationInterrupted => Ok(None),
            inquire::InquireError::NotTTY
            | inquire::InquireError::InvalidConfiguration(_)
            | inquire::InquireError::IO(_)
            | inquire::InquireError::Custom(_) => Err(e),
        })?;

    if let Some(selected_dummy) = selection_res {
        println!("{}", selected_dummy.gen())
    }

    Ok(())
}

fn minimal_select<'a, T: std::fmt::Display>(items: Vec<T>) -> Select<'a, T> {
    Select::new("", items)
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
pub enum Dummy {
    Uuidv4,
    Uuidv7,
    Email,
    UserAgent,
    IPv4,
    IPv6,
    MACAddress,
}

impl Dummy {
    pub fn gen(&self) -> String {
        match self {
            Dummy::Uuidv4 => fake::uuid::UUIDv4.fake::<String>(),
            Dummy::Uuidv7 => fake::uuid::UUIDv7.fake::<String>(),
            Dummy::Email => fake::faker::internet::en::SafeEmail().fake::<String>(),
            Dummy::UserAgent => fake::faker::internet::en::UserAgent().fake::<String>(),
            Dummy::MACAddress => fake::faker::internet::en::MACAddress().fake::<String>(),
            Dummy::IPv4 => fake::faker::internet::en::IPv4().fake::<String>(),
            Dummy::IPv6 => fake::faker::internet::en::IPv6().fake::<String>(),
        }
    }
}
