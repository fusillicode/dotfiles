use std::borrow::Cow;

use color_eyre::owo_colors::OwoColorize;
use fake::Fake;
use strum::Display;
use strum::EnumIter;
use strum::IntoEnumIterator;
use utils::sk::SkimItem;
use utils::sk::SkimItemPreview;
use utils::sk::SkimPreviewContext;

#[derive(EnumIter, Display, Clone, Copy, Debug)]
pub enum FkrOption {
    Uuidv4,
    Uuidv7,
    Email,
    UserAgent,
    IPv4,
    IPv6,
    MACAddress,
}

impl SkimItem for FkrOption {
    fn text(&self) -> Cow<'_, str> {
        Cow::from(self.to_string())
    }

    fn preview(&self, _context: SkimPreviewContext) -> SkimItemPreview {
        SkimItemPreview::AnsiText(format!("Generate a fake {self}").bold().to_string())
    }
}

impl FkrOption {
    /// Generates a fake string based on the [FkrOption] variant.
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

    /// Returns a [Vec] of all [FkrOption] variants.
    pub fn to_vec() -> Vec<Self> {
        FkrOption::iter().collect()
    }
}
