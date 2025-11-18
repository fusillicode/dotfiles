use nvim_oxi::Dictionary;

pub fn dict() -> Dictionary {
    dict! {
        "get_diff_lines": fn_from!(get_diff_lines),
    }
}

fn get_diff_lines(_: ()) {
    let Ok(git_diff_output) =
        ytil_git::diff::get().inspect_err(|err| ytil_nvim_oxi::api::notify_error(format!("{err}")))
    else {
        return;
    };

    let Ok(paths_with_lnums) = ytil_git::diff::get_paths_with_lnums(&git_diff_output)
        .map(|out| {
            out.into_iter()
                .map(|(path, lnum)| (path.to_owned(), lnum))
                .collect::<Vec<_>>()
        })
        .inspect_err(|err| ytil_nvim_oxi::api::notify_error(format!("{err}")))
    else {
        return;
    };

    let displayable_choices = paths_with_lnums
        .iter()
        .map(|(path, lnum)| format!("{path}:{lnum}"))
        .collect::<Vec<_>>();

    let callback = {
        let paths_with_lnums = paths_with_lnums.clone();
        move |choice_idx: usize| {
            let Some((path, lnum)) = paths_with_lnums.get(choice_idx) else {
                return;
            };
            let _ = ytil_nvim_oxi::buffer::open(path, Some(*lnum), None)
                .inspect_err(|err| ytil_nvim_oxi::api::notify_error(format!("{err}")));
        }
    };

    if let Err(err) = ytil_nvim_oxi::api::vim_ui_select(displayable_choices, &[("prompt", "Git diff lines ")], callback)
    {
        ytil_nvim_oxi::api::notify_error(format!("error selecting git diff lines | error={err:#?}"));
    }
}
