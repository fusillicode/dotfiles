//! Exposes a dictionary with a `get_hunks` function that fetches git diff output, extracts paths and line numbers
//! of changed hunks, and presents a selection UI to jump to specific diff locations in buffers.

use nvim_oxi::Dictionary;
use ytil_nvim_oxi::api::QuickfixConfig;

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

    let mut all_items = vec![];
    for (path, lnum) in hunks.iter() {
        let Ok(lnum) = i64::try_from(*lnum) else {
            ytil_nvim_oxi::api::notify_error(format!("error converting hunk lnum to i64 | lnum={lnum}"));
            return;
        };
        all_items.push((path.clone(), lnum));
    }

    let quickfix = QuickfixConfig {
        trigger_value: "All to quickfix".into(),
        all_items,
    };

    let mut displayable_hunks = vec![quickfix.trigger_value.to_owned()];
    displayable_hunks.extend(
        hunks
            .iter()
            .map(|(path, lnum)| format!("{path}:{lnum}"))
            .collect::<Vec<_>>(),
    );

    let callback = {
        move |choice_idx: usize| {
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

    if let Err(err) = ytil_nvim_oxi::api::vim_ui_select(
        displayable_hunks,
        &[("prompt", "Git diff hunks ")],
        callback,
        Some(quickfix),
    ) {
        ytil_nvim_oxi::api::notify_error(format!("error opening selected path | error={err:#?}"));
    }
}
