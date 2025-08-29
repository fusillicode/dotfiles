//! Fake data generation library for the fkr tool.
//! Provides an enum of fake data types using the `fake` crate.

use std::borrow::Cow;

use color_eyre::owo_colors::OwoColorize as _;
use fake::Fake;
use strum::Display;
use strum::EnumIter;
use strum::IntoEnumIterator;
use utils::sk::SkimItem;
use utils::sk::SkimItemPreview;
use utils::sk::SkimPreviewContext;

/// Available fake data types for generation.
#[derive(EnumIter, Display, Clone, Copy, Debug)]
pub enum FkrOption {
    /// Generates a version 4 UUID (random)
    Uuidv4,
    /// Generates a version 7 UUID (timestamp-based)
    Uuidv7,
    /// Generates a realistic email address
    Email,
    /// Generates a browser user agent string
    UserAgent,
    /// Generates an IPv4 address
    IPv4,
    /// Generates an IPv6 address
    IPv6,
    /// Generates a MAC address
    MACAddress,
}

impl SkimItem for FkrOption {
    /// Returns the display text for the skim selection interface.
    fn text(&self) -> Cow<'_, str> {
        Cow::from(self.to_string())
    }

    /// Returns a preview of what will be generated for this option.
    fn preview(&self, _context: SkimPreviewContext) -> SkimItemPreview {
        SkimItemPreview::AnsiText(format!("Generate a fake {self}").bold().to_string())
    }
}

impl FkrOption {
    /// Generates a fake string value based on the selected variant.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use fkr::FkrOption;
    ///
    /// let email = FkrOption::Email.gen_string();
    /// ```
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

    /// Returns a vector of all available [FkrOption] variants.
    pub fn to_vec() -> Vec<Self> {
        FkrOption::iter().collect()
    }
}
