use nvim_oxi::Dictionary;

#[allow(dead_code)]
pub fn dict() -> Dictionary {
    dict! {
        "get_link": fn_from!(get_link),
        "get_blame_link": fn_from!(get_blame_link),
    }
}

fn get_link(_: Option<()>) {}

fn get_blame_link(_: Option<()>) {}
