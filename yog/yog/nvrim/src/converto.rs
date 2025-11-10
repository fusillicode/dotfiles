use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use strum::EnumIter;
use strum::IntoEnumIterator;

pub fn dict() -> Dictionary {
    dict! {
        "convert_selection": fn_from!(convert_selection),
    }
}

fn convert_selection(_: ()) {
    let Some(selection) = ytil_nvim_oxi::visual_selection::get(()) else {
        return;
    };

    let opts = Opt::iter();

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
                    return Ok(());
                };

                Buffer::from(selection.buf_id())
                    .set_text(
                        selection.line_range(),
                        selection.start().col,
                        selection.end().col,
                        vec![transformed_line],
                    )
                    .inspect_err(|error| {
                        ytil_nvim_oxi::api::notify_error(format!(
                            "cannot set lines of buffer | start={:#?} end={:#?} error={error:#?}",
                            selection.start(),
                            selection.end()
                        ));
                    })
            });
        }
    };

    if let Err(error) = ytil_nvim_oxi::api::vim_ui_select(opts, &[("prompt", "Select conversion ")], callback) {
        ytil_nvim_oxi::api::notify_error(error);
    }
}

#[derive(strum::Display, EnumIter)]
enum Opt {
    #[strum(to_string = "RGB to HEX")]
    RgbToHex,
    #[strum(to_string = "dd-mm-yyyy,hh:mm:ss to DateTime<Utc>")]
    DateTimeStrToChronoDateTime,
}

impl Opt {
    pub fn convert(&self, selection: &str) -> color_eyre::Result<String> {
        match self {
            Self::RgbToHex => rgb_to_hex(selection),
            Self::DateTimeStrToChronoDateTime => Ok("bar".into()),
        }
    }
}

fn rgb_to_hex(input: &str) -> color_eyre::Result<String> {
    fn u8_color_code_from_rgb_split(rgb: &mut std::str::Split<'_, &str>, color: &str) -> color_eyre::Result<u8> {
        rgb.next()
            .ok_or_else(|| eyre!("missing color component {color}"))
            .and_then(|s| {
                s.trim()
                    .parse::<u8>()
                    .map_err(|error| eyre!("cannot parse str as u8 color code | str={s:?} error={error:?}"))
            })
    }

    let mut rgb_split = input.split(",");
    let r = u8_color_code_from_rgb_split(&mut rgb_split, "R")?;
    let g = u8_color_code_from_rgb_split(&mut rgb_split, "G")?;
    let b = u8_color_code_from_rgb_split(&mut rgb_split, "B")?;

    Ok(format!("#{r:02x}{g:02x}{b:02x}"))
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
        assert2::let_assert!(Err(e) = result);
        pretty_assertions::assert_eq!(e.to_string(), expected_error);
    }
}
