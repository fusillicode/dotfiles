#![feature(exit_status_error)]

use fake::faker::internet::en::IPv4;
use fake::faker::internet::en::IPv6;
use fake::faker::internet::en::SafeEmail;
use fake::uuid::UUIDv4;
use fake::Fake;
use inquire::ui::RenderConfig;
use inquire::Select;
use strum::IntoEnumIterator;
use strum_macros::Display;
use strum_macros::EnumIter;

fn main() -> anyhow::Result<()> {
    let render_config = RenderConfig::default_colored()
        .with_prompt_prefix("".into())
        .with_canceled_prompt_indicator("".into())
        .with_answered_prompt_prefix("".into());

    Ok(Select::new("", Dummy::iter().collect())
        .with_render_config(render_config)
        .without_help_message()
        .prompt()
        .map(|selected_dummy| println!("{}", selected_dummy.gen()))
        .or_else(|e| match e {
            inquire::InquireError::OperationCanceled
            | inquire::InquireError::OperationInterrupted => Ok(()),
            inquire::InquireError::NotTTY
            | inquire::InquireError::InvalidConfiguration(_)
            | inquire::InquireError::IO(_)
            | inquire::InquireError::Custom(_) => Err(e),
        })?)
}

#[derive(EnumIter, Display)]
pub enum Dummy {
    Uuidv4,
    Email,
    IPv4,
    IPv6,
}

impl Dummy {
    pub fn gen(&self) -> String {
        match self {
            Dummy::Uuidv4 => UUIDv4.fake::<String>(),
            Dummy::Email => SafeEmail().fake::<String>(),
            Dummy::IPv4 => IPv4().fake::<String>(),
            Dummy::IPv6 => IPv6().fake::<String>(),
        }
    }
}
