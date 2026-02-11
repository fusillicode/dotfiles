//! Diagnostic processing utilities for LSP diagnostics.
//!
//! This module provides functionality to filter, format, and sort LSP diagnostics
//! received from language servers in Nvim.

use core::fmt;

use nvim_oxi::Dictionary;
use serde::de::Deserializer;
use serde::de::Visitor;
use strum::EnumCount;
use strum::EnumIter;

mod config;
mod filter;
mod filters;
mod formatter;
mod sorter;

/// [`Dictionary`] of diagnostic processing helpers.
pub fn dict() -> Dictionary {
    dict! {
        "filter": fn_from!(filter::filter),
        "sort": fn_from!(sorter::sort),
        "format": fn_from!(formatter::format),
        "config": config::get()
    }
}

/// Diagnostic severity levels.
///
/// Variant order defines iteration order via [`EnumIter`] for stable rendering.
#[derive(Clone, Copy, Debug, EnumCount, EnumIter, Eq, Hash, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum DiagnosticSeverity {
    Error,
    Warn,
    Info,
    Hint,
    Other,
}

impl DiagnosticSeverity {
    /// Number of declared severity variants.
    pub const VARIANT_COUNT: usize = <Self as strum::EnumCount>::COUNT;

    /// Returns the canonical LSP severity number (1-4, or 0 for Other).
    pub const fn to_number(self) -> u8 {
        match self {
            Self::Error => 1,
            Self::Warn => 2,
            Self::Info => 3,
            Self::Hint => 4,
            Self::Other => 0,
        }
    }

    /// Returns the canonical single-character symbol for this severity.
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Error => "E",
            Self::Warn => "W",
            Self::Info => "I",
            Self::Hint => "H",
            Self::Other => "",
        }
    }
}

/// Deserializes numeric, string, or text alias severity representations.
impl<'de> serde::Deserialize<'de> for DiagnosticSeverity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SevVisitor;

        impl Visitor<'_> for SevVisitor {
            type Value = DiagnosticSeverity;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a severity: 1-4 or (error|warn|info|hint) or short alias")
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(match v {
                    1 => DiagnosticSeverity::Error,
                    2 => DiagnosticSeverity::Warn,
                    3 => DiagnosticSeverity::Info,
                    4 => DiagnosticSeverity::Hint,
                    _ => DiagnosticSeverity::Other,
                })
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v < 0 {
                    return Ok(DiagnosticSeverity::Other);
                }
                self.visit_u64(v.cast_unsigned())
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let norm = s.trim().to_ascii_lowercase();
                if let Ok(n) = norm.parse::<u64>() {
                    return self.visit_u64(n);
                }
                Ok(match norm.as_str() {
                    "error" | "err" | "e" => DiagnosticSeverity::Error,
                    "warn" | "warning" | "w" => DiagnosticSeverity::Warn,
                    "info" | "information" | "i" => DiagnosticSeverity::Info,
                    "hint" | "h" => DiagnosticSeverity::Hint,
                    _ => DiagnosticSeverity::Other,
                })
            }
        }

        deserializer.deserialize_any(SevVisitor)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    #[rstest]
    #[case("1", DiagnosticSeverity::Error)]
    #[case("2", DiagnosticSeverity::Warn)]
    #[case("3", DiagnosticSeverity::Info)]
    #[case("4", DiagnosticSeverity::Hint)]
    #[case("\"1\"", DiagnosticSeverity::Error)]
    #[case("\"2\"", DiagnosticSeverity::Warn)]
    #[case("\"3\"", DiagnosticSeverity::Info)]
    #[case("\"4\"", DiagnosticSeverity::Hint)]
    #[case("\"error\"", DiagnosticSeverity::Error)]
    #[case("\"err\"", DiagnosticSeverity::Error)]
    #[case("\"e\"", DiagnosticSeverity::Error)]
    #[case("\"warn\"", DiagnosticSeverity::Warn)]
    #[case("\"warning\"", DiagnosticSeverity::Warn)]
    #[case("\"w\"", DiagnosticSeverity::Warn)]
    #[case("\"info\"", DiagnosticSeverity::Info)]
    #[case("\"information\"", DiagnosticSeverity::Info)]
    #[case("\"i\"", DiagnosticSeverity::Info)]
    #[case("\"hint\"", DiagnosticSeverity::Hint)]
    #[case("\"h\"", DiagnosticSeverity::Hint)]
    #[case("\" Error \"", DiagnosticSeverity::Error)]
    #[case("\"WARNING\"", DiagnosticSeverity::Warn)]
    #[case("\" Info \"", DiagnosticSeverity::Info)]
    #[case("\"H\"", DiagnosticSeverity::Hint)]
    #[case("-1", DiagnosticSeverity::Other)]
    #[case("0", DiagnosticSeverity::Other)]
    #[case("5", DiagnosticSeverity::Other)]
    #[case("\"unknown\"", DiagnosticSeverity::Other)]
    fn diagnostic_severity_deserializes_strings_as_expected(#[case] input: &str, #[case] expected: DiagnosticSeverity) {
        assert2::let_assert!(Ok(sev) = serde_json::from_str::<DiagnosticSeverity>(input));
        assert_eq!(sev, expected);
    }

    #[test]
    fn diagnostic_severity_when_iterated_via_enumiter_yields_declared_order_and_matches_variant_count() {
        let expected = [
            DiagnosticSeverity::Error,
            DiagnosticSeverity::Warn,
            DiagnosticSeverity::Info,
            DiagnosticSeverity::Hint,
            DiagnosticSeverity::Other,
        ];
        let collected: Vec<DiagnosticSeverity> = DiagnosticSeverity::iter().collect();
        pretty_assertions::assert_eq!(collected.as_slice(), expected.as_slice());
        pretty_assertions::assert_eq!(collected.len(), DiagnosticSeverity::VARIANT_COUNT);
    }

    #[rstest]
    #[case(1_i64, DiagnosticSeverity::Error)]
    #[case(2_i64, DiagnosticSeverity::Warn)]
    #[case(3_i64, DiagnosticSeverity::Info)]
    #[case(4_i64, DiagnosticSeverity::Hint)]
    #[case(-1_i64, DiagnosticSeverity::Other)]
    #[case(0_i64, DiagnosticSeverity::Other)]
    #[case(5_i64, DiagnosticSeverity::Other)]
    fn diagnostic_severity_deserializes_numeric_values_as_expected(
        #[case] input: i64,
        #[case] expected: DiagnosticSeverity,
    ) {
        let json = input.to_string();
        assert2::let_assert!(Ok(sev) = serde_json::from_str::<DiagnosticSeverity>(&json));
        assert_eq!(sev, expected);
    }

    #[test]
    fn diagnostic_severity_deserializes_invalid_json_errors() {
        assert2::let_assert!(Err(err) = serde_json::from_str::<DiagnosticSeverity>("error"));
        let msg = err.to_string();
        assert!(msg.contains("expected value"), "unexpected error message: {msg}");
    }
}
