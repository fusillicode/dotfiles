use std::collections::BTreeMap;

use muxr_core::ClientKey;
use muxr_core::ClientKeyCode;
use muxr_core::ClientKeyModifiers;

const DEFAULT_LOCAL_KEYBINDINGS: [(KeyChord, LocalAction); 2] = [
    (
        KeyChord {
            code: char_code('C'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        LocalAction::CopySelection,
    ),
    (
        KeyChord {
            code: char_code('X'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        LocalAction::CopySelectionInline,
    ),
];

const DEFAULT_NORMAL_KEYBINDINGS: [(KeyChord, NormalAction); 24] = [
    (
        KeyChord {
            code: char_code('N'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusNextTab,
    ),
    (
        KeyChord {
            code: char_code('P'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPreviousTab,
    ),
    (
        KeyChord {
            code: char_code('n'),
            modifiers: KeyModifiers::CTRL_ALT,
        },
        NormalAction::MoveTabRight,
    ),
    (
        KeyChord {
            code: char_code('p'),
            modifiers: KeyModifiers::CTRL_ALT,
        },
        NormalAction::MoveTabLeft,
    ),
    (
        KeyChord {
            code: char_code('1'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab1,
    ),
    (
        KeyChord {
            code: char_code('2'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab2,
    ),
    (
        KeyChord {
            code: char_code('3'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab3,
    ),
    (
        KeyChord {
            code: char_code('4'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab4,
    ),
    (
        KeyChord {
            code: char_code('5'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab5,
    ),
    (
        KeyChord {
            code: char_code('6'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab6,
    ),
    (
        KeyChord {
            code: char_code('7'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab7,
    ),
    (
        KeyChord {
            code: char_code('8'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab8,
    ),
    (
        KeyChord {
            code: char_code('9'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab9,
    ),
    (
        KeyChord {
            code: char_code('E'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::CreateTab,
    ),
    (
        KeyChord {
            code: char_code('H'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPaneLeft,
    ),
    (
        KeyChord {
            code: char_code('J'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPaneDown,
    ),
    (
        KeyChord {
            code: char_code('K'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPaneUp,
    ),
    (
        KeyChord {
            code: char_code('L'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPaneRight,
    ),
    (
        KeyChord {
            code: char_code('D'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::SplitPaneBottom,
    ),
    (
        KeyChord {
            code: char_code('V'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::SplitPaneRight,
    ),
    (
        KeyChord {
            code: char_code('W'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::ClosePane,
    ),
    (
        KeyChord {
            code: char_code('F'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::TogglePaneFullscreen,
    ),
    (
        KeyChord {
            code: char_code('R'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::EnterResizeMode,
    ),
    (
        KeyChord {
            code: char_code('S'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::OpenScrollbackEditor,
    ),
];

const DEFAULT_RESIZE_KEYBINDINGS: [(KeyChord, ResizeAction); 9] = [
    (
        KeyChord {
            code: SupportedKeyCode::Esc,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ExitResizeMode,
    ),
    (
        KeyChord {
            code: char_code('h'),
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneLeft,
    ),
    (
        KeyChord {
            code: SupportedKeyCode::Left,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneLeft,
    ),
    (
        KeyChord {
            code: char_code('j'),
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneDown,
    ),
    (
        KeyChord {
            code: SupportedKeyCode::Down,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneDown,
    ),
    (
        KeyChord {
            code: char_code('k'),
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneUp,
    ),
    (
        KeyChord {
            code: SupportedKeyCode::Up,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneUp,
    ),
    (
        KeyChord {
            code: char_code('l'),
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneRight,
    ),
    (
        KeyChord {
            code: SupportedKeyCode::Right,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneRight,
    ),
];

/// The server input mode whose keybindings are being resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeybindingMode {
    Normal,
    Resize,
}

/// Semantic actions available to the compiled server keymap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeybindingAction {
    ClosePane,
    CreateTab,
    EnterResizeMode,
    ExitResizeMode,
    FocusNextTab,
    FocusPaneDown,
    FocusPaneLeft,
    FocusPaneRight,
    FocusPaneUp,
    FocusPreviousTab,
    FocusTab1,
    FocusTab2,
    FocusTab3,
    FocusTab4,
    FocusTab5,
    FocusTab6,
    FocusTab7,
    FocusTab8,
    FocusTab9,
    MoveTabLeft,
    MoveTabRight,
    OpenScrollbackEditor,
    ResizePaneDown,
    ResizePaneLeft,
    ResizePaneRight,
    ResizePaneUp,
    SplitPaneBottom,
    SplitPaneRight,
    TogglePaneFullscreen,
}

/// Semantic actions available to the compiled client-local keymap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalKeybindingAction {
    CopySelection,
    CopySelectionInline,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AsciiChar(u8);

impl AsciiChar {
    const fn from_client_character(character: char) -> Option<Self> {
        if character.is_ascii() && !character.is_ascii_control() {
            Some(Self(character as u8))
        } else {
            None
        }
    }

    const fn from_binding_character(character: char) -> Self {
        assert!(character.is_ascii() && !character.is_ascii_control());
        Self(character as u8)
    }

    const fn byte(self) -> u8 {
        self.0
    }

    const fn canonical(self, modifiers: KeyModifiers) -> Self {
        let byte = match modifiers {
            KeyModifiers::None => self.0,
            KeyModifiers::Alt | KeyModifiers::Ctrl | KeyModifiers::CtrlAlt => self.0.to_ascii_lowercase(),
            KeyModifiers::Shift | KeyModifiers::ShiftAlt | KeyModifiers::CtrlShift | KeyModifiers::CtrlAltShift => {
                match self.0 {
                    b'!' => b'1',
                    b'@' => b'2',
                    b'#' => b'3',
                    b'$' => b'4',
                    b'%' => b'5',
                    b'^' => b'6',
                    b'&' => b'7',
                    b'*' => b'8',
                    b'(' => b'9',
                    b')' => b'0',
                    b'_' => b'-',
                    b'+' => b'=',
                    b'{' => b'[',
                    b'}' => b']',
                    b'|' => b'\\',
                    b':' => b';',
                    b'"' => b'\'',
                    b'<' => b',',
                    b'>' => b'.',
                    b'?' => b'/',
                    b'~' => b'`',
                    byte => byte,
                }
                .to_ascii_uppercase()
            }
        };
        Self(byte)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SupportedKeyCode {
    Char(AsciiChar),
    Down,
    Esc,
    Left,
    Right,
    Up,
}

impl SupportedKeyCode {
    const fn canonical(self, modifiers: KeyModifiers) -> Self {
        match self {
            Self::Char(character) => Self::Char(character.canonical(modifiers)),
            Self::Down => Self::Down,
            Self::Esc => Self::Esc,
            Self::Left => Self::Left,
            Self::Right => Self::Right,
            Self::Up => Self::Up,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum KeyModifiers {
    None,
    Alt,
    Ctrl,
    CtrlAlt,
    Shift,
    ShiftAlt,
    CtrlShift,
    CtrlAltShift,
}

impl KeyModifiers {
    const CTRL_ALT: Self = Self::CtrlAlt;
    const NONE: Self = Self::None;
    const SHIFT_ALT: Self = Self::ShiftAlt;

    const fn from_client_modifiers(modifiers: ClientKeyModifiers) -> Self {
        match (modifiers.alt, modifiers.ctrl, modifiers.shift) {
            (false, false, false) => Self::NONE,
            (true, false, false) => Self::Alt,
            (false, true, false) => Self::Ctrl,
            (true, true, false) => Self::CTRL_ALT,
            (false, false, true) => Self::Shift,
            (true, false, true) => Self::SHIFT_ALT,
            (false, true, true) => Self::CtrlShift,
            (true, true, true) => Self::CtrlAltShift,
        }
    }
}

const fn char_code(character: char) -> SupportedKeyCode {
    SupportedKeyCode::Char(AsciiChar::from_binding_character(character))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyCharacterSupport {
    Alt,
    AltAndShiftAlt,
    ShiftAlt,
    Unsupported,
}

// These variants mirror the bytes that the legacy decoder can turn into an Alt or Shift-Alt key. The legacy escape
// prefix for `[` and `]` starts a control sequence, so neither byte can represent an Alt chord.
const fn legacy_character_support(byte: u8) -> LegacyCharacterSupport {
    match byte {
        b'a'..=b'z' | b' ' => LegacyCharacterSupport::Alt,
        b'A'..=b'Z' | b'[' | b']' => LegacyCharacterSupport::ShiftAlt,
        b'0'..=b'9' | b'-' | b'=' | b'\\' | b';' | b'\'' | b',' | b'.' | b'/' | b'`' => {
            LegacyCharacterSupport::AltAndShiftAlt
        }
        _ => LegacyCharacterSupport::Unsupported,
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct KeyChord {
    code: SupportedKeyCode,
    modifiers: KeyModifiers,
}

impl KeyChord {
    const fn from_client_key(key: &ClientKey) -> Option<Self> {
        let code = match key.code {
            ClientKeyCode::Char(character) => {
                let Some(character) = AsciiChar::from_client_character(character) else {
                    return None;
                };
                SupportedKeyCode::Char(character)
            }
            ClientKeyCode::Down => SupportedKeyCode::Down,
            ClientKeyCode::Esc => SupportedKeyCode::Esc,
            ClientKeyCode::Left => SupportedKeyCode::Left,
            ClientKeyCode::Right => SupportedKeyCode::Right,
            ClientKeyCode::Up => SupportedKeyCode::Up,
            ClientKeyCode::Backspace | ClientKeyCode::Enter | ClientKeyCode::Tab | ClientKeyCode::Unknown => {
                return None;
            }
        };
        let modifiers = KeyModifiers::from_client_modifiers(key.modifiers);
        Some(Self {
            code: code.canonical(modifiers),
            modifiers,
        })
    }

    const fn canonical(self) -> Self {
        Self {
            code: self.code.canonical(self.modifiers),
            modifiers: self.modifiers,
        }
    }

    const fn comparison(self, other: Self) -> KeyChordComparison {
        let same_code = match (self.code, other.code) {
            (SupportedKeyCode::Char(left), SupportedKeyCode::Char(right)) => left.byte() == right.byte(),
            (SupportedKeyCode::Down, SupportedKeyCode::Down)
            | (SupportedKeyCode::Esc, SupportedKeyCode::Esc)
            | (SupportedKeyCode::Left, SupportedKeyCode::Left)
            | (SupportedKeyCode::Right, SupportedKeyCode::Right)
            | (SupportedKeyCode::Up, SupportedKeyCode::Up) => true,
            _ => false,
        };
        let same_modifiers = matches!(
            (self.modifiers, other.modifiers),
            (KeyModifiers::None, KeyModifiers::None)
                | (KeyModifiers::Alt, KeyModifiers::Alt)
                | (KeyModifiers::Ctrl, KeyModifiers::Ctrl)
                | (KeyModifiers::CtrlAlt, KeyModifiers::CtrlAlt)
                | (KeyModifiers::Shift, KeyModifiers::Shift)
                | (KeyModifiers::ShiftAlt, KeyModifiers::ShiftAlt)
                | (KeyModifiers::CtrlShift, KeyModifiers::CtrlShift)
                | (KeyModifiers::CtrlAltShift, KeyModifiers::CtrlAltShift)
        );
        match (same_code, same_modifiers) {
            (true, true) => KeyChordComparison::Same,
            _ => KeyChordComparison::Different,
        }
    }

    const fn canonical_comparison(self, other: Self) -> KeyChordComparison {
        self.canonical().comparison(other.canonical())
    }

    const fn validation(self) -> KeyChordValidation {
        match self.comparison(self.canonical()) {
            KeyChordComparison::Same => {}
            KeyChordComparison::Different => return KeyChordValidation::NonCanonical,
        }

        match (self.code, self.modifiers) {
            (
                SupportedKeyCode::Char(_)
                | SupportedKeyCode::Down
                | SupportedKeyCode::Esc
                | SupportedKeyCode::Left
                | SupportedKeyCode::Right
                | SupportedKeyCode::Up,
                KeyModifiers::None,
            ) => KeyChordValidation::Supported,
            (SupportedKeyCode::Char(character), KeyModifiers::Alt) => {
                match legacy_character_support(character.byte()) {
                    LegacyCharacterSupport::Alt | LegacyCharacterSupport::AltAndShiftAlt => {
                        KeyChordValidation::Supported
                    }
                    LegacyCharacterSupport::ShiftAlt | LegacyCharacterSupport::Unsupported => {
                        KeyChordValidation::Unsupported
                    }
                }
            }
            (SupportedKeyCode::Char(character), KeyModifiers::ShiftAlt) => {
                match legacy_character_support(character.byte()) {
                    LegacyCharacterSupport::ShiftAlt | LegacyCharacterSupport::AltAndShiftAlt => {
                        KeyChordValidation::Supported
                    }
                    LegacyCharacterSupport::Alt | LegacyCharacterSupport::Unsupported => {
                        KeyChordValidation::Unsupported
                    }
                }
            }
            (SupportedKeyCode::Char(character), KeyModifiers::CtrlAlt) if matches!(character.byte(), b'n' | b'p') => {
                KeyChordValidation::Supported
            }
            _ => KeyChordValidation::Unsupported,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyChordComparison {
    Different,
    Same,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyChordValidation {
    NonCanonical,
    Supported,
    Unsupported,
}

const _: () = assert_unique_keybindings(&DEFAULT_LOCAL_KEYBINDINGS);
const _: () = assert_unique_keybindings(&DEFAULT_NORMAL_KEYBINDINGS);
const _: () = assert_unique_keybindings(&DEFAULT_RESIZE_KEYBINDINGS);
const _: () = assert_disjoint_keybindings(&DEFAULT_LOCAL_KEYBINDINGS, &DEFAULT_NORMAL_KEYBINDINGS);
const _: () = assert_disjoint_keybindings(&DEFAULT_LOCAL_KEYBINDINGS, &DEFAULT_RESIZE_KEYBINDINGS);

const fn assert_unique_keybindings<Action>(bindings: &[(KeyChord, Action)]) {
    let mut remaining = bindings;
    while let Some((first, rest)) = remaining.split_first() {
        let validation = first.0.validation();
        assert!(
            !matches!(validation, KeyChordValidation::NonCanonical),
            "non-canonical muxr keybinding"
        );
        assert!(
            matches!(validation, KeyChordValidation::Supported),
            "unsupported muxr keybinding"
        );
        let mut comparison = rest;
        while let Some((next, rest)) = comparison.split_first() {
            assert!(
                matches!(first.0.canonical_comparison(next.0), KeyChordComparison::Different),
                "duplicate muxr keybinding"
            );
            comparison = rest;
        }
        remaining = rest;
    }
}

const fn assert_disjoint_keybindings<LeftAction, RightAction>(
    left: &[(KeyChord, LeftAction)],
    right: &[(KeyChord, RightAction)],
) {
    let mut left = left;
    while let Some((first, rest)) = left.split_first() {
        let mut right = right;
        while let Some((other, rest)) = right.split_first() {
            assert!(
                matches!(first.0.canonical_comparison(other.0), KeyChordComparison::Different),
                "cross-namespace muxr keybinding collision"
            );
            right = rest;
        }
        left = rest;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalAction {
    CopySelection,
    CopySelectionInline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NormalAction {
    ClosePane,
    CreateTab,
    EnterResizeMode,
    FocusNextTab,
    FocusPaneDown,
    FocusPaneLeft,
    FocusPaneRight,
    FocusPaneUp,
    FocusPreviousTab,
    FocusTab1,
    FocusTab2,
    FocusTab3,
    FocusTab4,
    FocusTab5,
    FocusTab6,
    FocusTab7,
    FocusTab8,
    FocusTab9,
    MoveTabLeft,
    MoveTabRight,
    OpenScrollbackEditor,
    SplitPaneBottom,
    SplitPaneRight,
    TogglePaneFullscreen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResizeAction {
    ExitResizeMode,
    ResizePaneDown,
    ResizePaneLeft,
    ResizePaneRight,
    ResizePaneUp,
}

impl From<LocalAction> for LocalKeybindingAction {
    fn from(action: LocalAction) -> Self {
        match action {
            LocalAction::CopySelection => Self::CopySelection,
            LocalAction::CopySelectionInline => Self::CopySelectionInline,
        }
    }
}

impl From<NormalAction> for KeybindingAction {
    fn from(action: NormalAction) -> Self {
        match action {
            NormalAction::ClosePane => Self::ClosePane,
            NormalAction::CreateTab => Self::CreateTab,
            NormalAction::EnterResizeMode => Self::EnterResizeMode,
            NormalAction::FocusNextTab => Self::FocusNextTab,
            NormalAction::FocusPaneDown => Self::FocusPaneDown,
            NormalAction::FocusPaneLeft => Self::FocusPaneLeft,
            NormalAction::FocusPaneRight => Self::FocusPaneRight,
            NormalAction::FocusPaneUp => Self::FocusPaneUp,
            NormalAction::FocusPreviousTab => Self::FocusPreviousTab,
            NormalAction::FocusTab1 => Self::FocusTab1,
            NormalAction::FocusTab2 => Self::FocusTab2,
            NormalAction::FocusTab3 => Self::FocusTab3,
            NormalAction::FocusTab4 => Self::FocusTab4,
            NormalAction::FocusTab5 => Self::FocusTab5,
            NormalAction::FocusTab6 => Self::FocusTab6,
            NormalAction::FocusTab7 => Self::FocusTab7,
            NormalAction::FocusTab8 => Self::FocusTab8,
            NormalAction::FocusTab9 => Self::FocusTab9,
            NormalAction::MoveTabLeft => Self::MoveTabLeft,
            NormalAction::MoveTabRight => Self::MoveTabRight,
            NormalAction::OpenScrollbackEditor => Self::OpenScrollbackEditor,
            NormalAction::SplitPaneBottom => Self::SplitPaneBottom,
            NormalAction::SplitPaneRight => Self::SplitPaneRight,
            NormalAction::TogglePaneFullscreen => Self::TogglePaneFullscreen,
        }
    }
}

impl From<ResizeAction> for KeybindingAction {
    fn from(action: ResizeAction) -> Self {
        match action {
            ResizeAction::ExitResizeMode => Self::ExitResizeMode,
            ResizeAction::ResizePaneDown => Self::ResizePaneDown,
            ResizeAction::ResizePaneLeft => Self::ResizePaneLeft,
            ResizeAction::ResizePaneRight => Self::ResizePaneRight,
            ResizeAction::ResizePaneUp => Self::ResizePaneUp,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Keymap<Action> {
    bindings: BTreeMap<KeyChord, Action>,
}

impl<Action, const LENGTH: usize> From<[(KeyChord, Action); LENGTH]> for Keymap<Action> {
    fn from(bindings: [(KeyChord, Action); LENGTH]) -> Self {
        Self {
            bindings: bindings.into_iter().collect(),
        }
    }
}

impl<Action: Copy> Keymap<Action> {
    fn resolve(&self, chord: KeyChord) -> Option<Action> {
        self.bindings.get(&chord).copied()
    }
}

/// Compiled keybinding tables shared by the muxr client and server.
///
/// The default inventory is compiled in. Normal mode uses Shift-Option-N/P for tab focus, Control-Option-N/P for tab
/// movement, Shift-Option-1 through 9 for tab selection, Shift-Option-E for tab creation, Shift-Option-H/J/K/L for
/// pane focus, Shift-Option-D/V for bottom/right splits, Shift-Option-W for pane close, Shift-Option-F for fullscreen,
/// Shift-Option-R for resize mode, and Shift-Option-S for scrollback. Resize mode uses Esc and h/j/k/l or the arrow
/// keys. Local mode uses Shift-Option-C/X for the two copy actions. Entries use decoder-canonical character forms.
/// Only chords representable by both Kitty keyboard input and the legacy decoder are compiled. Changes require
/// rebuilding muxr.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeybindingsConfig {
    local: Keymap<LocalAction>,
    normal: Keymap<NormalAction>,
    resize: Keymap<ResizeAction>,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            local: Keymap::from(DEFAULT_LOCAL_KEYBINDINGS),
            normal: Keymap::from(DEFAULT_NORMAL_KEYBINDINGS),
            resize: Keymap::from(DEFAULT_RESIZE_KEYBINDINGS),
        }
    }
}

impl KeybindingsConfig {
    /// Resolve a normalized client key in the client-local keymap.
    pub fn resolve_local(&self, key: &ClientKey) -> Option<LocalKeybindingAction> {
        let chord = KeyChord::from_client_key(key)?;
        self.local.resolve(chord).map(LocalKeybindingAction::from)
    }

    /// Resolve a normalized client key in one compiled server keymap.
    pub fn resolve(&self, mode: KeybindingMode, key: &ClientKey) -> Option<KeybindingAction> {
        let chord = KeyChord::from_client_key(key)?;
        match mode {
            KeybindingMode::Normal => self.normal.resolve(chord).map(KeybindingAction::from),
            KeybindingMode::Resize => self.resize.resolve(chord).map(KeybindingAction::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use muxr_core::ClientKey;
    use muxr_core::ClientKeyCode;
    use muxr_core::ClientKeyModifiers;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_keybindings_default_when_contains_unique_inventory_returns_config() {
        let keybindings = KeybindingsConfig::default();

        assert_that!(keybindings.local.bindings.len(), eq(2));
        assert_that!(keybindings.normal.bindings.len(), eq(24));
        assert_that!(keybindings.resize.bindings.len(), eq(9));
    }

    #[rstest::rstest]
    #[case::copy(
        ClientKeyCode::Char('C'),
        ClientKeyModifiers::SHIFT_ALT,
        LocalKeybindingAction::CopySelection
    )]
    #[case::copy_lowercase(
        ClientKeyCode::Char('c'),
        ClientKeyModifiers::SHIFT_ALT,
        LocalKeybindingAction::CopySelection
    )]
    #[case::inline_copy(
        ClientKeyCode::Char('X'),
        ClientKeyModifiers::SHIFT_ALT,
        LocalKeybindingAction::CopySelectionInline
    )]
    fn test_keybindings_resolve_local_when_default_chord_arrives_returns_action(
        #[case] code: ClientKeyCode,
        #[case] modifiers: ClientKeyModifiers,
        #[case] action: LocalKeybindingAction,
    ) {
        let keybindings = KeybindingsConfig::default();
        let key = ClientKey {
            code,
            modifiers,
            raw_bytes: Vec::new(),
        };

        assert_that!(keybindings.resolve_local(&key), some(eq(action)));
    }

    #[rstest::rstest]
    #[case::resize_arrow(
        KeybindingMode::Resize,
        ClientKeyCode::Left,
        ClientKeyModifiers::NONE,
        KeybindingAction::ResizePaneLeft
    )]
    #[case::resize_vi(
        KeybindingMode::Resize,
        ClientKeyCode::Char('h'),
        ClientKeyModifiers::NONE,
        KeybindingAction::ResizePaneLeft
    )]
    #[case::focus_tab_nine(
        KeybindingMode::Normal,
        ClientKeyCode::Char('9'),
        ClientKeyModifiers::SHIFT_ALT,
        KeybindingAction::FocusTab9
    )]
    fn test_keybindings_resolve_when_default_chord_arrives_returns_action(
        #[case] mode: KeybindingMode,
        #[case] code: ClientKeyCode,
        #[case] modifiers: ClientKeyModifiers,
        #[case] action: KeybindingAction,
    ) {
        let keybindings = KeybindingsConfig::default();
        let key = ClientKey {
            code,
            modifiers,
            raw_bytes: Vec::new(),
        };

        assert_that!(keybindings.resolve(mode, &key), some(eq(action)));
    }

    #[test]
    fn test_keybindings_resolve_when_unsupported_code_arrives_returns_none() {
        let keybindings = KeybindingsConfig::default();
        let key = ClientKey {
            code: ClientKeyCode::Enter,
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: Vec::new(),
        };

        assert_that!(keybindings.resolve(KeybindingMode::Normal, &key), none());
    }

    #[test]
    fn test_keybindings_resolve_when_shifted_punctuation_arrives_returns_tab_action() {
        let keybindings = KeybindingsConfig::default();
        let key = ClientKey {
            code: ClientKeyCode::Char('!'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: Vec::new(),
        };

        assert_that!(
            keybindings.resolve(KeybindingMode::Normal, &key),
            some(eq(KeybindingAction::FocusTab1))
        );
    }

    #[test]
    fn test_key_chord_when_shifted_letter_case_varies_matches_canonical_chord() {
        let lower = KeyChord {
            code: char_code('c'),
            modifiers: KeyModifiers::SHIFT_ALT,
        };
        let upper = KeyChord {
            code: char_code('C'),
            modifiers: KeyModifiers::SHIFT_ALT,
        };

        assert_that!(lower.canonical_comparison(upper), eq(KeyChordComparison::Same));
        assert_that!(lower.validation(), eq(KeyChordValidation::NonCanonical));
        assert_that!(upper.validation(), eq(KeyChordValidation::Supported));
    }

    #[rstest::rstest]
    #[case::alt_left_bracket(KeyChord { code: char_code('['), modifiers: KeyModifiers::Alt })]
    #[case::alt_right_bracket(KeyChord { code: char_code(']'), modifiers: KeyModifiers::Alt })]
    #[case::alt_shifted_punctuation(KeyChord { code: char_code('!'), modifiers: KeyModifiers::Alt })]
    #[case::ctrl_alt_other_character(KeyChord { code: char_code('a'), modifiers: KeyModifiers::CtrlAlt })]
    #[case::shifted_arrow(KeyChord { code: SupportedKeyCode::Left, modifiers: KeyModifiers::Shift })]
    fn test_key_chord_when_legacy_decoder_cannot_emit_chord_reports_unsupported(#[case] chord: KeyChord) {
        assert_that!(chord.validation(), eq(KeyChordValidation::Unsupported));
    }
}
