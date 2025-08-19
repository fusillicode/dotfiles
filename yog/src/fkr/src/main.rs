#![feature(exit_status_error)]

use fkr::FkrOption;

/// Prints on terminal a fake value generated on the fly based on what was selected.
/// If "cp" is supplied as first argument also copies the generated value to the system clipboard.
/// If the selection is cancelled (<ESC>) or interrupted (<CTRL-C>) exists with success without
/// printing anything.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let generated_value = match utils::sk::get_item(FkrOption::to_vec())? {
        Some(fkr_option) => fkr_option.gen_string(),
        None => return Ok(()),
    };

    println!("{generated_value}");

    if utils::system::get_args()
        .first()
        .is_some_and(|arg| arg == "cp")
    {
        utils::system::cp_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}
