use fake::Fake;
use fake::faker::internet::en::IPv4;
use fake::faker::internet::en::IPv6;
use fake::faker::internet::en::MACAddress;
use fake::faker::internet::en::SafeEmail;
use fake::faker::internet::en::UserAgent;
use fake::uuid::UUIDv4;
use fake::uuid::UUIDv7;
use strum::EnumIter;
use strum::IntoEnumIterator;

/// Available fake data types for generation.
#[derive(Clone, Copy, Debug, strum::Display, EnumIter)]
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
    /// Generates a fake string value.
    pub fn gen_string(&self) -> String {
        match self {
            Self::Uuidv4 => UUIDv4.fake::<String>(),
            Self::Uuidv7 => UUIDv7.fake::<String>(),
            Self::Email => SafeEmail().fake::<String>(),
            Self::UserAgent => UserAgent().fake::<String>(),
            Self::MACAddress => MACAddress().fake::<String>(),
            Self::IPv4 => IPv4().fake::<String>(),
            Self::IPv6 => IPv6().fake::<String>(),
        }
    }

    /// Returns all available variants.
    pub fn to_vec() -> Vec<Self> {
        Self::iter().collect()
    }
}
