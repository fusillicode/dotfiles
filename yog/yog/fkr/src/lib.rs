use fake::Fake;
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
            Self::Uuidv4 => fake::uuid::UUIDv4.fake::<String>(),
            Self::Uuidv7 => fake::uuid::UUIDv7.fake::<String>(),
            Self::Email => fake::faker::internet::en::SafeEmail().fake::<String>(),
            Self::UserAgent => fake::faker::internet::en::UserAgent().fake::<String>(),
            Self::MACAddress => fake::faker::internet::en::MACAddress().fake::<String>(),
            Self::IPv4 => fake::faker::internet::en::IPv4().fake::<String>(),
            Self::IPv6 => fake::faker::internet::en::IPv6().fake::<String>(),
        }
    }

    /// Returns all available variants.
    pub fn to_vec() -> Vec<Self> {
        Self::iter().collect()
    }
}
