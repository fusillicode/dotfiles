use inquire::Autocomplete;
use inquire::Text;

use crate::tui::minimal_render_config;

pub fn minimal<'a, T: std::fmt::Display>(ac: Option<Box<dyn Autocomplete>>) -> Text<'a> {
    let mut text = Text::new("").with_render_config(minimal_render_config());
    text.autocompleter = ac;
    text
}
