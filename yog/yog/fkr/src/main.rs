#![feature(exit_status_error)]

use fkr::FkrOption;

/// Interactive fake data generator.
/// Prints generated value to stdout, optionally copies to clipboard.
///
/// # Arguments
///
/// * `cp` - Copy generated value to clipboard
///
/// # Errors
///
/// Returns an error if:
/// - An underlying operation fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let Some(generated_value) = inquire::minimal_select(FkrOption::to_vec())?.map(|fkr_opt| fkr_opt.gen_string())
    else {
        return Ok(());
    };

    println!("{generated_value}");

    if system::get_args().first().is_some_and(|arg| arg == "cp") {
        system::cp_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}
