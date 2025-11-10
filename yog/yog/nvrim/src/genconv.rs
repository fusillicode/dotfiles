//! General conversions helpers for the current Visual selection.
//!
//! Provides a namespaced [`Dictionary`] exposing selection conversion
//! functionality (RGB to HEX and date/time to chrono parse code).

use std::str::Split;

use chrono::DateTime;
use chrono::NaiveDate;
use chrono::NaiveDateTime;
use chrono::NaiveTime;
use color_eyre::eyre::Report;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use strum::EnumIter;
use strum::IntoEnumIterator;

/// Namespaced dictionary of general conversion helpers.
///
/// Entries:
/// - `"convert_selection"`: wraps [`convert_selection`] and converts the active Visual selection using a user-selected
///   conversion option.
pub fn dict() -> Dictionary {
    dict! {
        "convert_selection": fn_from!(convert_selection),
    }
}

/// Converts the current visual selection using a user-chosen conversion option.
///
/// Prompts the user (via [`ytil_nvim_oxi::api::vim_ui_select`]) to select a conversion
/// option, then applies the conversion to the selected text in place.
///
/// Returns early if:
/// - No active Visual selection is detected.
/// - The user cancels the prompt.
/// - The conversion fails (an error is reported via [`ytil_nvim_oxi::api::notify_error`]).
/// - Writing the converted text back to the buffer fails (an error is reported via
///   [`ytil_nvim_oxi::api::notify_error`]).
///
/// # Returns
/// Returns `()` upon successful completion.
///
/// # Errors
/// Errors from [`ytil_nvim_oxi::api::vim_ui_select`] are reported via [`ytil_nvim_oxi::api::notify_error`]
/// using the direct display representation of [`color_eyre::Report`].
/// Conversion errors are also reported similarly.
///
/// # Notes
/// Currently supports single-line selections; multiline could be added later.
fn convert_selection(_: ()) {
    let Some(selection) = ytil_nvim_oxi::visual_selection::get(()) else {
        return;
    };

    let opts = ConversionOption::iter();

    let callback = {
        let opts = opts.clone();
        move |choice_idx| {
            opts.get(choice_idx).map(|opt| {
                let Ok(transformed_line) = opt
                    // Conversion should work only with 1 single line but maybe multiline could be
                    // supported at some point.
                    .convert(&selection.lines().to_vec().join("\n"))
                    .inspect_err(|error| {
                        ytil_nvim_oxi::api::notify_error(format!(
                            "cannot set lines of buffer | start={:#?} end={:#?} error={error:#?}",
                            selection.start(),
                            selection.end()
                        ));
                    })
                else {
                    return Ok::<(), Report>(());
                };
                ytil_nvim_oxi::buffer::replace_text_and_notify_if_error(&selection, vec![transformed_line]);
                Ok(())
            });
        }
    };

    if let Err(error) = ytil_nvim_oxi::api::vim_ui_select(opts, &[("prompt", "Select conversion ")], callback) {
        ytil_nvim_oxi::api::notify_error(error);
    }
}

/// Enum representing available conversion options.
#[derive(strum::Display, EnumIter)]
enum ConversionOption {
    /// Converts RGB color values to hexadecimal format.
    #[strum(to_string = "RGB to HEX")]
    RgbToHex,
    /// Converts date/time strings to chrono `parse_from_str` code.
    #[strum(to_string = "Datetime formatted strings to chrono parse_from_str code")]
    DateTimeStrToChronoParseFromStr,
}

impl ConversionOption {
    pub fn convert(&self, selection: &str) -> color_eyre::Result<String> {
        match self {
            Self::RgbToHex => rgb_to_hex(selection),
            Self::DateTimeStrToChronoParseFromStr => date_time_str_to_chrono_parse_from_str(selection),
        }
    }
}

/// Converts an RGB string to a hexadecimal color code.
///
/// Expects an input in the format of [`u8`] R, G, B values.
/// Whitespaces around components are trimmed.
///
/// # Returns
/// Returns the hexadecimal color code as a string (e.g., "#ff0000").
///
/// # Errors
/// Returns an error if the input format is invalid or components cannot be parsed as u8.
fn rgb_to_hex(input: &str) -> color_eyre::Result<String> {
    fn u8_color_code_from_rgb_split(rgb: &mut Split<'_, char>, color: &str) -> color_eyre::Result<u8> {
        rgb.next()
            .ok_or_else(|| eyre!("missing color component {color}"))
            .and_then(|s| {
                s.trim()
                    .parse::<u8>()
                    .map_err(|error| eyre!("cannot parse str as u8 color code | str={s:?} error={error:?}"))
            })
    }

    let mut rgb_split = input.split(',');
    let r = u8_color_code_from_rgb_split(&mut rgb_split, "R")?;
    let g = u8_color_code_from_rgb_split(&mut rgb_split, "G")?;
    let b = u8_color_code_from_rgb_split(&mut rgb_split, "B")?;

    Ok(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// Converts a date/time string to the appropriate chrono `parse_from_str` code snippet.
///
/// Attempts to parse the input with various chrono types and formats:
/// - [`DateTime`] with offset
/// - [`NaiveDateTime`]
/// - [`NaiveDate`]
/// - [`NaiveTime`]
///
/// # Returns
/// Returns a string containing the Rust code for parsing the input with chrono.
///
/// # Errors
/// Returns an error if the input cannot be parsed with any supported format.
fn date_time_str_to_chrono_parse_from_str(input: &str) -> color_eyre::Result<String> {
    if DateTime::parse_from_str(input, "%d-%m-%Y,%H:%M:%S%z").is_ok() {
        return Ok(format!(r#"DateTime::parse_from_str("{input}", "%d-%m-%Y,%H:%M:%S%Z")"#));
    }
    if NaiveDateTime::parse_from_str(input, "%d-%m-%Y,%H:%M:%S").is_ok() {
        return Ok(format!(
            r#"NaiveDateTime::parse_from_str("{input}", "%d-%m-%Y,%H:%M:%S")"#
        ));
    }
    if NaiveDate::parse_from_str(input, "%d-%m-%Y").is_ok() {
        return Ok(format!(r#"NaiveDate::parse_from_str("{input}", "%d-%m-%Y")"#));
    }
    if NaiveTime::parse_from_str(input, "%H:%M:%S").is_ok() {
        return Ok(format!(r#"NaiveTime::parse_from_str("{input}", "%H:%M:%S")"#));
    }
    Err(eyre!(
        "cannot get chrono parse_from_str for supplied input | input={input:?}"
    ))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::red("255,0,0", "#ff0000")]
    #[case::red_with_spaces(" 255 , 0 , 0 ", "#ff0000")]
    #[case::black("0,0,0", "#000000")]
    #[case::white("255,255,255", "#ffffff")]
    #[case::red_with_extra_component("255,0,0,123", "#ff0000")]
    fn rgb_to_hex_when_valid_rgb_returns_hex(#[case] input: &str, #[case] expected: &str) {
        assert2::let_assert!(Ok(actual) = rgb_to_hex(input));
        pretty_assertions::assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::empty_input(
        "",
        "cannot parse str as u8 color code | str=\"\" error=ParseIntError { kind: Empty }"
    )]
    #[case::single_component("0", "missing color component G")]
    #[case::two_components("255,0", "missing color component B")]
    #[case::out_of_range_red(
        "256,0,0",
        "cannot parse str as u8 color code | str=\"256\" error=ParseIntError { kind: PosOverflow }"
    )]
    #[case::invalid_green(
        "255,abc,0",
        "cannot parse str as u8 color code | str=\"abc\" error=ParseIntError { kind: InvalidDigit }"
    )]
    #[case::invalid_blue(
        "255,0,def",
        "cannot parse str as u8 color code | str=\"def\" error=ParseIntError { kind: InvalidDigit }"
    )]
    fn rgb_to_hex_when_invalid_input_returns_error(#[case] input: &str, #[case] expected_error: &str) {
        let result = rgb_to_hex(input);
        assert2::let_assert!(Err(error) = result);
        pretty_assertions::assert_eq!(error.to_string(), expected_error);
    }

    #[rstest]
    #[case::datetime_with_offset(
        "25-12-2023,14:30:45+00:00",
        r#"DateTime::parse_from_str("25-12-2023,14:30:45+00:00", "%d-%m-%Y,%H:%M:%S%Z")"#
    )]
    #[case::naive_datetime(
        "25-12-2023,14:30:45",
        r#"NaiveDateTime::parse_from_str("25-12-2023,14:30:45", "%d-%m-%Y,%H:%M:%S")"#
    )]
    #[case::naive_date("25-12-2023", r#"NaiveDate::parse_from_str("25-12-2023", "%d-%m-%Y")"#)]
    #[case::naive_time("14:30:45", r#"NaiveTime::parse_from_str("14:30:45", "%H:%M:%S")"#)]
    fn date_time_str_to_chrono_parse_from_str_when_valid_input_returns_correct_code(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        assert2::let_assert!(Ok(actual) = date_time_str_to_chrono_parse_from_str(input));
        pretty_assertions::assert_eq!(actual, expected);
    }

    #[test]
    fn date_time_str_to_chrono_parse_from_str_when_invalid_input_returns_error() {
        assert2::let_assert!(Err(error) = date_time_str_to_chrono_parse_from_str("invalid"));
        pretty_assertions::assert_eq!(
            error.to_string(),
            "cannot get chrono parse_from_str for supplied input | input=\"invalid\""
        );
    }
}
