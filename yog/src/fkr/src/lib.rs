//! Fake data generation library for the fkr tool.
//!
//! This library provides an enum of different fake data types that can be generated
//! using the `fake` crate. Each variant represents a different type of test data
//! commonly needed during development and testing.

use std::borrow::Cow;

use color_eyre::owo_colors::OwoColorize;
use fake::Fake;
use strum::Display;
use strum::EnumIter;
use strum::IntoEnumIterator;
use utils::sk::SkimItem;
use utils::sk::SkimItemPreview;
use utils::sk::SkimPreviewContext;

/// Enumeration of available fake data types that can be generated.
///
/// Each variant represents a different type of test data commonly used in development
/// and testing scenarios. The enum implements various traits to support interactive
/// selection and display in terminal interfaces.
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
    /// Uses the `fake` crate to generate realistic test data for each option type.
    /// The generated values are suitable for use in development, testing, and
    /// placeholder data scenarios.
    ///
    /// # Returns
    ///
    /// A `String` containing the generated fake data.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use fkr::FkrOption;
    ///
    /// let email = FkrOption::Email.gen_string();
    /// let uuid = FkrOption::Uuidv4.gen_string();
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

    /// Returns a vector containing all available `FkrOption` variants.
    ///
    /// This is useful for creating selection interfaces or iterating over
    /// all available fake data types.
    ///
    /// # Returns
    ///
    /// A `Vec<FkrOption>` containing all enum variants in declaration order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use fkr::FkrOption;
    ///
    /// let all_options = FkrOption::to_vec();
    /// assert_eq!(all_options.len(), 7);
    /// ```
    pub fn to_vec() -> Vec<Self> {
        FkrOption::iter().collect()
    }
}
