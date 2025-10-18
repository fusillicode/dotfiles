#![feature(exit_status_error)]

//! Generate fake data strings from an enum façade over selected [`fake`] providers.
//!
//! Provides a single variant enum ([`FkrOption`]) with a uniform `gen_string` method for
//! quick ad‑hoc values (UUIDs, emails, IPs, user agents) without pulling individual faker
//! types into every caller.
//!
//! # Arguments
//! - `cp` Optional flag to copy the generated value to clipboard.
//!
//! # Usage
//! ```bash
//! fkr # select a generator; prints value
//! fkr cp # select -> prints -> copies to clipboard
//! ```
//!
//! # Errors
//! - Interactive selection UI fails.
//! - Writing the generated value to the clipboard fails.

use fake::Fake;
use strum::EnumIter;
use strum::IntoEnumIterator;

/// The actual `main` inner logic.
///
/// # Errors
/// - Interactive selection UI fails.
/// - Writing the generated value to the clipboard fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let Some(generated_value) = ytil_tui::minimal_select(FkrOption::to_vec())?.map(|fkr_opt| fkr_opt.gen_string())
    else {
        return Ok(());
    };

    println!("{generated_value}");

    if ytil_system::get_args().first().is_some_and(|arg| arg == "cp") {
        ytil_system::cp_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}

/// Available fake data types for generation.
#[derive(Clone, Copy, Debug, strum::Display, EnumIter)]
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
    /// Generates an MAC address
    MACAddress,
}

impl FkrOption {
    /// Generates a fake string value based on the selected variant.
    pub fn gen_string(&self) -> String {
        match self {
            Self::Uuidv4 => fake::uuid::UUIDv4.fake::<String>(),
            Self::Uuidv7 => fake::uuid::UUIDv7.fake::<String>(),
            Self::Email => fake::faker::internet::en::SafeEmail().fake::<String>(),
            Self::UserAgent => fake::faker::internet::en::UserAgent().fake::<String>(),
            Self::MACAddress => fake::faker::internet::en::MACAddress().fake::<String>(),
            Self::IPv4 => fake::faker::internet::en::IPv4().fake::<String>(),
            Self::IPv6 => fake::faker::internet::en::IPv6().fake::<String>(),
        }
    }

    /// Returns a vector of all available [`FkrOption`] variants.
    pub fn to_vec() -> Vec<Self> {
        Self::iter().collect()
    }
}
