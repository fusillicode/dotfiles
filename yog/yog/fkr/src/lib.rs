//! Generate fake data strings from an enum façade over selected [`fake`] providers.
//!
//! Provides a single variant enum ([`FkrOption`]) with a uniform `gen_string` method for
//! quick ad‑hoc values (UUIDs, emails, IPs, user agents) without pulling individual faker
//! types into every caller.

use fake::Fake;
use strum::Display;
use strum::EnumIter;
use strum::IntoEnumIterator;

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
