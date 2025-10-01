//! Interactively generate fake data (optionally copy to clipboard).
#![feature(exit_status_error)]

use fkr::FkrOption;

/// Interactive fake data generator.
/// Prints generated value to standard output, optionally copies to clipboard.
///
/// # Usage
///
/// ```bash
/// fkr            # interactive choose + print
/// fkr cp         # as above, additionally copy to system clipboard
/// ```
///
/// # Arguments
///
/// * `cp` - Copy generated value to clipboard (optional)
///
/// # Errors
/// In case:
/// - Interactive selection fails.
/// - Writing the generated value to the clipboard fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let Some(generated_value) = ytil_tui::minimal_select(FkrOption::to_vec())?.map(|fkr_opt| fkr_opt.gen_string())
    else {
        return Ok(());
    };

    println!("{generated_value}");

    if ytil_system::get_args().first().is_some_and(|arg| arg == "cp") {
        ytil_system::cp_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}
