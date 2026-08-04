//! Askama custom templates filters.
//!
//! Provides helpers to format various types in Askama templates.

#![allow(
    clippy::unnecessary_wraps,
    clippy::inline_always,
    clippy::unused_self,
    reason = "askama filter signature is framework-defined"
)]

use jiff::Timestamp;

/// Format a [`Timestamp`] as ISO-8601 / RFC3339 (UTC, whole seconds).
#[askama::filter_fn]
pub fn format_to_iso_8601(timestamp: &Timestamp, _args: &dyn askama::Values) -> askama::Result<String> {
    Ok(timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
}

#[cfg(test)]
mod tests {
    use askama::Template;
    use jiff::Timestamp;
    use test_that::prelude::*;

    mod filters {
        pub use crate::templates::filters::format_to_iso_8601;
    }

    #[test]
    fn test_format_to_iso_8601_when_datetime_valid_returns_iso_8601_string() {
        #[derive(Template)]
        #[template(source = "{{ value | format_to_iso_8601 }}", ext = "txt")]
        struct DummyFilterTemplate {
            value: Timestamp,
        }

        let dummy_filter_template = DummyFilterTemplate {
            value: Timestamp::from_second(1_735_787_045).unwrap(),
        };
        assert_that!(dummy_filter_template.render(), ok(eq("2025-01-02T03:04:05Z")));
    }
}
