//! Askama custom templates filters.
//!
//! Provides helpers to format various types in Askama templates.
#![allow(clippy::unnecessary_wraps)]

use chrono::DateTime;
use chrono::SecondsFormat;
use chrono::Utc;

/// Format a [`DateTime<Utc>`] as ISO-8601 / RFC3339 (UTC, whole seconds)
///
/// Produces strings like `2025-10-01T14:37:22Z`.
///
/// # Arguments
/// * `dt` - UTC timestamp to format.
///
/// # Returns
/// RFC3339 / ISO‑8601 string with `Z` (UTC) designator, no fractional seconds.
pub fn format_to_iso_8601(dt: &DateTime<Utc>, _args: &dyn askama::Values) -> askama::Result<String> {
    Ok(dt.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn format_to_iso_8601_worsk_as_expected() {
        let ts = Utc.with_ymd_and_hms(2025, 1, 2, 3, 4, 5).unwrap();
        let res = format_to_iso_8601(&ts, &()).unwrap();
        assert_eq!(res, "2025-01-02T03:04:05Z");
    }
}
