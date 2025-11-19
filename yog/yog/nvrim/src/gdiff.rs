//! Exposes a dictionary with a `get_hunks` function that fetches git diff output, extracts paths and line numbers
//! of changed hunks, and presents a selection UI to jump to specific diff locations in buffers.

use nvim_oxi::Dictionary;

const ALL_QUICKFIX_CHOICE: &str = "All to quickfix";

/// [`Dictionary`] of git diff helpers.
pub fn dict() -> Dictionary {
    dict! {
        "get_hunks": fn_from!(get_hunks),
    }
}

/// Opens the selected git diff file and line number.
///
/// Fetches git diff output, parses paths and line numbers of changed hunks, displays them in a Neovim selection UI, and
/// on selection opens the buffer at the specified line.
fn get_hunks(_: ()) {
    let Ok(raw_output) = ytil_git::diff::get_raw().inspect_err(|err| {
        ytil_nvim_oxi::api::notify_error(format!("error getting git diff raw output | error={err:#?}"));
    }) else {
        return;
    };

    let Ok(hunks) = ytil_git::diff::get_hunks(&raw_output)
        .map(|x| {
            x.into_iter()
                .map(|(path, lnum)| (path.to_owned(), lnum))
                .collect::<Vec<_>>()
        })
        .inspect_err(|err| {
            ytil_nvim_oxi::api::notify_error(format!("error getting git diff hunks | error={err:#?}"));
        })
    else {
        return;
    };

    let mut displayable_hunks = vec![ALL_QUICKFIX_CHOICE.to_owned()];
    displayable_hunks.extend(
        hunks
            .iter()
            .map(|(path, lnum)| format!("{path}:{lnum}"))
            .collect::<Vec<_>>(),
    );

    let callback = {
        let displayable_hunks = displayable_hunks.clone();
        move |choice_idx: usize| {
            if displayable_hunks
                .get(choice_idx)
                .is_some_and(|s| s == ALL_QUICKFIX_CHOICE)
            {
                let all_hunks = hunks
                    .iter()
                    .map(|(path, lnum)| (path.as_str(), *lnum))
                    .collect::<Vec<_>>();
                let _ = ytil_nvim_oxi::api::open_quickfix(&all_hunks).inspect_err(|err| {
                    ytil_nvim_oxi::api::notify_error(format!(" | error={err:#?}"));
                });
                return;
            }
            let Some((path, lnum)) = hunks.get(choice_idx) else {
                return;
            };
            let _ = ytil_nvim_oxi::buffer::open(path, Some(*lnum), None).inspect_err(|err| {
                ytil_nvim_oxi::api::notify_error(format!(
                    "error opening buffer | path={path:?} lnum={lnum} error={err:#?}"
                ));
            });
        }
    };

    if let Err(err) = ytil_nvim_oxi::api::vim_ui_select(displayable_hunks, &[("prompt", "Git diff hunks ")], callback) {
        ytil_nvim_oxi::api::notify_error(format!("error opening selected path | error={err:#?}"));
    }
}
