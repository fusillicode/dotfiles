use nvim_oxi::Dictionary;

use crate::dict;
use crate::fn_from;

pub fn dict() -> Dictionary {
    dict! {
        "transform": fn_from!(transform),
    }
}

pub fn transform(_: ()) {}
