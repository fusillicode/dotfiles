use nvim_oxi::Dictionary;

mod diagnostics;
mod statusline;

#[nvim_oxi::plugin]
fn rua2() -> Dictionary {
    Dictionary::from_iter([
        ("format_diagnostic", diagnostics::formatter::format()),
        ("sort_diagnostics", diagnostics::sorter::sort()),
        ("draw_statusline", statusline::draw()),
    ])
}
