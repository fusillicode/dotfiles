//! Diagnostic processing utilities for LSP diagnostics.
//!
//! This module provides functionality to filter, format, and sort LSP diagnostics
//! received from language servers in Nvim.

use core::fmt;

use nvim_oxi::Dictionary;
use serde::de::Deserializer;
use serde::de::Visitor;
use strum::EnumIter;

use crate::dict;
use crate::fn_from;
use crate::oxi_ext::dict::DictionaryExt;

mod filter;
mod filters;
mod formatter;
mod sorter;

/// [`Dictionary`] of diagnostic processing helpers.
///
/// Includes:
/// - `format`: format function used by floating diagnostics window.
/// - `sort`: severity sorter (descending severity).
/// - `filter`: buffer / rules based filter.
/// - `config`: nested dictionary mirroring `vim.diagnostic.config({...})` currently defined in Lua.
pub fn dict() -> Dictionary {
    dict! {
        "format": fn_from!(formatter::format),
        "sort": fn_from!(sorter::sort),
        "filter": fn_from!(filter::filter),
        "config": dict! {
            "severity_sort": true,
            "signs": true,
            "underline": true,
            "update_in_insert": false,
            "virtual_text": false,
            "float": dict! {
                "anchor_bias": "above",
                "border": crate::style_opts::dict()
                    .get_dict(&["window"])
                    .unwrap_or_default()
                    .unwrap_or_default()
                    .get_t::<nvim_oxi::String>("border").unwrap_or_else(|_| "none".to_string()),
                "focusable": true,
                "format": fn_from!(formatter::format),
                "header": "",
                "prefix": "",
                "source": false,
                "suffix": "",
            }
        }
    }
}

/// Diagnostic severity levels.
///
/// See the `Deserialize` impl below for accepted serialized forms.
#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone, strum::Display, EnumIter)]
#[allow(clippy::upper_case_acronyms)]
pub enum DiagnosticSeverity {
    /// Error severity.
    #[strum(to_string = "e")]
    Error,
    /// Warning severity.
    #[strum(to_string = "w")]
    Warn,
    /// Info severity.
    #[strum(to_string = "i")]
    Info,
    /// Hint severity.
    #[strum(to_string = "h")]
    Hint,
    /// Any other / unknown severity value.
    Other,
}

impl DiagnosticSeverity {
    /// Returns the numeric representation of a given [`DiagnosticSeverity`].
    ///
    /// The representation is the canonical LSP severity:
    /// - 1 for Error
    /// - 2 for Warning
    /// - 3 for Information
    /// - 4 for Hint
    /// - 0 for Other to indicate an unmapped / unknown severity.
    ///
    /// # Returns
    /// - `u8` numeric code 1 to 4 for known severities, 0 for [`DiagnosticSeverity::Other`].
    ///
    /// # Rationale
    /// Avoids relying on implicit enum discriminant order via `as u8` casts,
    /// making the mapping explicit and resilient to future variant reordering
    /// or insertion. Using an inherent method keeps the API surface small while
    /// centralizing the mapping logic in one place.
    pub const fn to_number(self) -> u8 {
        match self {
            Self::Error => 1,
            Self::Warn => 2,
            Self::Info => 3,
            Self::Hint => 4,
            Self::Other => 0,
        }
    }
}

/// Deserializes accepted severity representations:
/// - Numeric u8
/// - Numeric strings
/// - Text aliases (case-insensitive)
/// - Any other value maps to [`DiagnosticSeverity::Other`].
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
