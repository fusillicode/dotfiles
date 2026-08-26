use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;

pub mod agent;

#[derive(Debug, Eq, PartialEq)]
pub enum ParseError {
    Missing(&'static str),
    Invalid { field: &'static str, value: String },
}

impl ParseError {
    pub fn invalid(field: &'static str, value: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            value: value.into(),
        }
    }
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(field) => write!(f, "missing {field}"),
            Self::Invalid { field, value } => write!(f, "invalid {field}: {value}"),
        }
    }
}

impl Error for ParseError {}
