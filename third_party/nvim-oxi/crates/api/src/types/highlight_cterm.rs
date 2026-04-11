use serde::{Deserialize, Serialize};

/// Terminal highlight attributes passed via `cterm` to
/// [`crate::opts::SetHighlightOptsBuilder::cterm`].
#[non_exhaustive]
#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Deserialize, Serialize,
)]
pub struct HighlightCterm {
    pub bold: bool,
    pub standout: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub undercurl: bool,
    pub underdouble: bool,
    pub underdotted: bool,
    pub underdashed: bool,
    pub italic: bool,
    pub reverse: bool,
    pub altfont: bool,
    pub dim: bool,
    pub blink: bool,
    pub conceal: bool,
    pub overline: bool,
    pub nocombine: bool,
}

impl From<HighlightCterm> for types::Dictionary {
    #[inline(always)]
    fn from(cterm: HighlightCterm) -> Self {
        Self::from_iter([
            ("bold", cterm.bold),
            ("standout", cterm.standout),
            ("strikethrough", cterm.strikethrough),
            ("underline", cterm.underline),
            ("undercurl", cterm.undercurl),
            ("underdouble", cterm.underdouble),
            ("underdotted", cterm.underdotted),
            ("underdashed", cterm.underdashed),
            ("italic", cterm.italic),
            ("reverse", cterm.reverse),
            ("altfont", cterm.altfont),
            ("dim", cterm.dim),
            ("blink", cterm.blink),
            ("conceal", cterm.conceal),
            ("overline", cterm.overline),
            ("nocombine", cterm.nocombine),
        ])
    }
}

impl From<HighlightCterm> for types::Object {
    #[inline(always)]
    fn from(cterm: HighlightCterm) -> Self {
        types::Dictionary::from(cterm).into()
    }
}
