//! Exposes a dictionary with a `get_hunks` function that fetches git diff output, extracts paths and line numbers
//! of changed hunks, and presents a selection UI to jump to specific diff locations in buffers.

use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use ytil_noxi::vim_ui_select::QuickfixConfig;

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
///
/// # Arguments
/// `only_current_buffer` If `Some(true)`, restricts the diff to only the current buffer's changes, If [`None`] or
/// `Some(false)`, shows all changed hunks across the repository.
fn get_hunks(only_current_buffer: Option<bool>) {
    let current_buffer_path = ytil_noxi::buffer::get_absolute_path(
        only_current_buffer
            .is_some_and(std::convert::identity)
            .then(Buffer::current)
            .as_ref(),
    );

    let Ok(raw_output) = ytil_git::diff::get_raw(current_buffer_path.as_deref()).inspect_err(|err| {
        ytil_noxi::notify::error(format!("error getting git diff raw output | error={err:#?}"));
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
            ytil_noxi::notify::error(format!("error getting git diff hunks | error={err:#?}"));
        })
    else {
        return;
    };

    let mut all_items = vec![];
    for (path, lnum) in &hunks {
        let Ok(lnum) = i64::try_from(*lnum) else {
            ytil_noxi::notify::error(format!("error converting hunk lnum to i64 | lnum={lnum}"));
            return;
        };
        all_items.push((path.clone(), lnum));
    }

    let quickfix = QuickfixConfig {
        trigger_value: "All to quickfix".into(),
        all_items,
    };

    let mut displayable_hunks: Vec<_> = hunks.iter().map(|(path, lnum)| format!("{path}:{lnum}")).collect();
    displayable_hunks.push(quickfix.trigger_value.clone());

    let callback = {
        move |choice_idx: usize| {
            let Some((path, lnum)) = hunks.get(choice_idx) else {
                return;
            };
            let _ = ytil_noxi::buffer::open(path, Some(*lnum), None).inspect_err(|err| {
                ytil_noxi::notify::error(format!(
                    "error opening buffer | path={path:?} lnum={lnum} error={err:#?}"
                ));
            });
        }
    };

    if let Err(err) = ytil_noxi::vim_ui_select::open(
        displayable_hunks,
        &[("prompt", "Git diff hunks ")],
        callback,
        Some(quickfix),
    ) {
        ytil_noxi::notify::error(format!("error opening selected path | error={err:#?}"));
    }
}
