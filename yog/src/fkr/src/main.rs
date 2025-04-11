#![feature(exit_status_error)]

use fkr::FkrOption;
use utils::tui::ClosablePrompt;
use utils::tui::ClosablePromptError;

/// Prints on terminal a fake value generated on the fly based on what was selected.
/// If "cp" is supplied as first argument also copies the generated value to the system clipboard.
/// If the selection is cancelled (<ESC>) or interrupted (<CTRL-C>) exists with success without
/// printing anything.
fn main() -> anyhow::Result<()> {
    let generated_value = match utils::tui::select::minimal(FkrOption::to_vec()).closable_prompt() {
        Ok(fkr_option) => fkr_option.gen_string(),
        Err(ClosablePromptError::Closed) => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    println!("{generated_value}");

    if utils::system::get_args()
        .first()
        .is_some_and(|arg| arg == "cp")
    {
        utils::system::copy_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}
