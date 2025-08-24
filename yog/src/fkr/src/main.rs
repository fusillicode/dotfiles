#![feature(exit_status_error)]

use fkr::FkrOption;

/// A fake data generator that provides various types of test data through an interactive interface.
///
/// This tool presents a selection menu of different fake data types and generates
/// a random value based on the user's choice. The generated value is printed to stdout
/// and can optionally be copied to the system clipboard.
///
/// # Arguments
///
/// * `cp` - Optional first argument that copies the generated value to clipboard
///
/// # Available Data Types
///
/// - UUIDv4: Generates a version 4 UUID
/// - UUIDv7: Generates a version 7 UUID
/// - Email: Generates a fake email address
/// - UserAgent: Generates a fake user agent string
/// - IPv4: Generates a fake IPv4 address
/// - IPv6: Generates a fake IPv6 address
/// - MACAddress: Generates a fake MAC address
///
/// # Examples
///
/// Generate fake data interactively:
/// ```bash
/// fkr
/// ```
///
/// Generate and copy to clipboard:
/// ```bash
/// fkr cp
/// ```
///
/// # Exit Behavior
///
/// - Returns success (0) if selection is cancelled with ESC or interrupted with Ctrl-C
/// - Returns success (0) after successfully generating and displaying a value
/// - Returns error code if generation fails
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let generated_value = match utils::sk::get_item(FkrOption::to_vec(), Default::default())? {
        Some(fkr_option) => fkr_option.gen_string(),
        None => return Ok(()),
    };

    println!("{generated_value}");

    if utils::system::get_args().first().is_some_and(|arg| arg == "cp") {
        utils::system::cp_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}
