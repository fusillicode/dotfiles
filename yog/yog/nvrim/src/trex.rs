use std::ops::Deref;

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

    let options: Vec<_> = Case::all_cases().iter().copied().map(CaseWrap).collect();
    let Ok(selected_option) = crate::oxi_ext::inputlist("Select option:", &options).inspect_err(|error| {
        crate::oxi_ext::notify_error(&format!("cannot user input, error {error:#?}"));
    }) else {
        return;
    };

    let transformed_lines = selection
        .lines()
        .iter()
        .map(|line| line.as_str().to_case(**selected_option))
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
        ));
    }
}

struct CaseWrap<'a>(Case<'a>);

impl<'a> core::fmt::Display for CaseWrap<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl<'a> Deref for CaseWrap<'a> {
    type Target = Case<'a>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
