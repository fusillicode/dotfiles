#![feature(exit_status_error)]

use fkr::FkrOption;
use utils::tui::CancellablePrompt;

/// Prints on terminal a fake value generated on the fly based on the selection.
/// If "cp" is supplied as first argument also copies the generated value to the system clipboard.
/// If the selection is cancelled (<ESC>) or interrupted (<CTRL-C>) exists with success without
/// printing anything.
fn main() -> anyhow::Result<()> {
    if let Some(generated_value) = utils::tui::select::minimal(FkrOption::to_vec())
        .cancellable_prompt()?
        .map(|fkr_opt| fkr_opt.gen_string())
    {
        println!("{generated_value}");
        if utils::system::get_args()
            .first()
            .is_some_and(|arg| arg == "cp")
        {
            utils::system::copy_to_system_clipboard(&mut generated_value.as_bytes())?;
        }
    }
    Ok(())
}
