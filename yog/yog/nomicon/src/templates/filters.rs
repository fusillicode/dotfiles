//! Askama custom templates filters.
//!
//! Provides helpers to format various types in Askama templates.

#![allow(clippy::unnecessary_wraps)]

use chrono::DateTime;
use chrono::SecondsFormat;
use chrono::Utc;

/// Format a [`DateTime<Utc>`] as ISO-8601 / RFC3339 (UTC, whole seconds).
#[askama::filter_fn]
pub fn format_to_iso_8601(dt: &DateTime<Utc>, _args: &dyn askama::Values) -> askama::Result<String> {
    Ok(dt.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests {
    use askama::Template;
    use chrono::TimeZone;

    use super::*;

    mod filters {
        pub use crate::templates::filters::format_to_iso_8601;
    }

    #[test]
    fn format_to_iso_8601_works_as_expected() {
        #[derive(Template)]
        #[template(source = "{{ value | format_to_iso_8601 }}", ext = "txt")]
        struct DummyFilterTemplate {
            value: DateTime<Utc>,
        }

        let dummy_filter_template = DummyFilterTemplate {
            value: Utc.with_ymd_and_hms(2025, 1, 2, 3, 4, 5).unwrap(),
        };
        assert2::let_assert!(Ok(res) = dummy_filter_template.render());
        pretty_assertions::assert_eq!(res, "2025-01-02T03:04:05Z");
    }
}
