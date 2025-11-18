//! Exposes a dictionary with a `get_diff_lines` function that fetches git diff output, extracts paths and line numbers
//! of changed lines, and presents a selection UI to jump to specific diff locations in buffers.

use nvim_oxi::Dictionary;

/// [`Dictionary`] of git diff helpers.
pub fn dict() -> Dictionary {
    dict! {
        "get_diff_lines": fn_from!(get_diff_lines),
    }
}

/// Opens the selected git diff file and line number.
///
/// Fetches git diff output, parses paths and line numbers of changed lines, displays them in a Neovim selection UI, and
/// on selection opens the buffer at the specified line.
fn get_diff_lines(_: ()) {
    let Ok(git_diff_output) = ytil_git::diff::get()
        .inspect_err(|err| ytil_nvim_oxi::api::notify_error(format!("error getting git diff output | error={err:#?}")))
    else {
        return;
    };

    let Ok(paths_lnums) = ytil_git::diff::get_paths_with_lnums(&git_diff_output)
        .map(|x| {
            x.into_iter()
                .map(|(path, lnum)| (path.to_owned(), lnum))
                .collect::<Vec<_>>()
        })
        .inspect_err(|err| {
            ytil_nvim_oxi::api::notify_error(format!("error getting git paths and lnums | error={err:#?}"));
        })
    else {
        return;
    };

    let displayable_paths_lnums = paths_lnums
        .iter()
        .map(|(path, lnum)| format!("{path}:{lnum}"))
        .collect::<Vec<_>>();

    let callback = {
        move |choice_idx: usize| {
            let Some((path, lnum)) = paths_lnums.get(choice_idx) else {
                return;
            };
            let _ = ytil_nvim_oxi::buffer::open(path, Some(*lnum), None).inspect_err(|err| {
                ytil_nvim_oxi::api::notify_error(format!(
                    "error opening buffer | path={path:?} lnum={lnum} error={err:#?}"
                ));
            });
        }
    };

    if let Err(err) =
        ytil_nvim_oxi::api::vim_ui_select(displayable_paths_lnums, &[("prompt", "Git diff lines ")], callback)
    {
        ytil_nvim_oxi::api::notify_error(format!("error opening selected path | error={err:#?}"));
    }
}
