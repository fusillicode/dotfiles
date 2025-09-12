use convert_case::Case;
use convert_case::Casing as _;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;

use crate::buffer::visual_selection;
use crate::dict;
use crate::fn_from;

pub fn dict() -> Dictionary {
    dict! {
        "transform_text": fn_from!(transform),
    }
}

pub fn transform(_: ()) {
    let Some(selection_with_bounds) = visual_selection::get_with_bounds(()) else {
        return;
    };

    let transformed_lines = selection_with_bounds
        .lines()
        .iter()
        .map(|line| line.to_string().to_case(Case::Upper))
        .collect::<Vec<_>>();

    if let Err(error) = Buffer::current().set_text(
        selection_with_bounds.lines_range(),
        selection_with_bounds.start().col,
        selection_with_bounds.end().col,
        transformed_lines,
    ) {
        crate::oxi_ext::notify_error(&format!(
            "cannot set lines of buffer between {:#?} and {:#?}, error {error:#?}",
            selection_with_bounds.start(),
            selection_with_bounds.end()
        ))
    }
}
