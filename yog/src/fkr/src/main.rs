#![feature(exit_status_error)]

use fkr::FkrOption;

/// Interactive fake data generator.
/// Prints generated value to stdout, optionally copies to clipboard.
///
/// # Arguments
///
/// * `cp` - Copy generated value to clipboard
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let generated_value = match utils::sk::get_item(FkrOption::to_vec(), Option::default())? {
        Some(fkr_option) => fkr_option.gen_string(),
        None => return Ok(()),
    };

    println!("{generated_value}");

    if utils::system::get_args().first().is_some_and(|arg| arg == "cp") {
        utils::system::cp_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}
