use nvim_oxi::Dictionary;

#[allow(dead_code)]
pub fn dict() -> Dictionary {
    dict! {
        "get_link": fn_from!(get_link),
        "get_blame_link": fn_from!(get_blame_link),
    }
}

#[allow(dead_code, unused)]
fn get_link(_: Option<()>) {
    let Some(selection) = ytil_nvim_oxi::visual_selection::get(()) else {
        return;
    };
    let line_range = selection.line_range();
}

fn get_blame_link(_: Option<()>) {}
