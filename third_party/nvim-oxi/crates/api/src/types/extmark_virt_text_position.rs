use serde::Deserialize;
use types as nvim;

#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Deserialize)]
/// Controls the positioning of the virtual text associated to an extmark.
#[serde(rename_all(deserialize = "snake_case"))]
pub enum ExtmarkVirtTextPosition {
    /// Right after the EOL character (default).
    Eol,

    /// Display right aligned at the EOL.
    EolRightAlign,

    /// Display over the specified column, without shifting the underlying
    /// text.
    Overlay,

    /// Display right aligned in the window.
    RightAlign,

    /// Display at the specified column, and shift the buffer text to the right
    /// as needed.
    Inline,

    /// Display at a fixed window column.
    WinCol,
}

impl From<ExtmarkVirtTextPosition> for nvim::String {
    fn from(pos: ExtmarkVirtTextPosition) -> Self {
        use ExtmarkVirtTextPosition::*;

        Self::from(match pos {
            Eol => "eol",
            EolRightAlign => "eol_right_align",
            Overlay => "overlay",
            RightAlign => "right_align",
            Inline => "inline",
            WinCol => "win_col",
        })
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use types::Object;
    use types::serde::Deserializer;

    use super::*;

    #[test]
    fn test_extmark_virt_text_position_deserializes_inline() {
        let pos = ExtmarkVirtTextPosition::deserialize(Deserializer::new(
            Object::from("inline"),
        ))
        .unwrap();

        assert_eq!(ExtmarkVirtTextPosition::Inline, pos);
    }

    #[test]
    fn test_extmark_virt_text_position_deserializes_eol_right_align() {
        let pos = ExtmarkVirtTextPosition::deserialize(Deserializer::new(
            Object::from("eol_right_align"),
        ))
        .unwrap();

        assert_eq!(ExtmarkVirtTextPosition::EolRightAlign, pos);
    }
}
