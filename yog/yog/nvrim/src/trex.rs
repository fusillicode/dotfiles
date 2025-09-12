use convert_case::Case;
use convert_case::Casing as _;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;

use crate::buffer::visual_selection;
use crate::dict;
use crate::fn_from;

pub fn dict() -> Dictionary {
    dict! {
        "transform_selection": fn_from!(transform_selection),
    }
}

pub fn transform_selection(_: ()) {
    let Some(selection) = visual_selection::get(()) else {
        return;
    };

    let transformed_lines = selection
        .lines()
        .iter()
        .map(|line| line.to_string().to_case(Case::Upper))
        .collect::<Vec<_>>();

    if let Err(error) = Buffer::from(selection.buf_id()).set_text(
        selection.line_range(),
        selection.start().col,
        selection.end().col,
        transformed_lines,
    ) {
        crate::oxi_ext::notify_error(&format!(
            "cannot set lines of buffer between {:#?} and {:#?}, error {error:#?}",
            selection.start(),
            selection.end()
        ))
    }
}
