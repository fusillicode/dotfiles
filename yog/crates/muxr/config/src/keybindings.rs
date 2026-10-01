use std::collections::BTreeMap;

use muxr_core::ClientKey;
use muxr_core::ClientKeyCode;
use muxr_core::ClientKeyModifiers;
use nutype::nutype;

const DEFAULT_LOCAL_KEYBINDINGS: [(KeyChordSpec, LocalAction); 2] = [
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('C'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        LocalAction::CopySelection,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('X'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        LocalAction::CopySelectionInline,
    ),
];

const DEFAULT_NORMAL_KEYBINDINGS: [(KeyChordSpec, NormalAction); 24] = [
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('N'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusNextTab,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('P'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPreviousTab,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('n'),
            modifiers: KeyModifiers::CTRL_ALT,
        },
        NormalAction::MoveTabRight,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('p'),
            modifiers: KeyModifiers::CTRL_ALT,
        },
        NormalAction::MoveTabLeft,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('1'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab1,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('2'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab2,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('3'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab3,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('4'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab4,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('5'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab5,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('6'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab6,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('7'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab7,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('8'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab8,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('9'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusTab9,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('E'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::CreateTab,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('H'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPaneLeft,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('J'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPaneDown,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('K'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPaneUp,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('L'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::FocusPaneRight,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('D'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::SplitPaneBottom,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('V'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::SplitPaneRight,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('W'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::ClosePane,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('F'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::TogglePaneFullscreen,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('R'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::EnterResizeMode,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('S'),
            modifiers: KeyModifiers::SHIFT_ALT,
        },
        NormalAction::OpenScrollbackEditor,
    ),
];

const DEFAULT_RESIZE_KEYBINDINGS: [(KeyChordSpec, ResizeAction); 9] = [
    (
        KeyChordSpec {
            code: KeyCodeSpec::Esc,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ExitResizeMode,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('h'),
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneLeft,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::Left,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneLeft,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('j'),
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneDown,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::Down,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneDown,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('k'),
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneUp,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::Up,
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneUp,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::from_character('l'),
            modifiers: KeyModifiers::NONE,
        },
        ResizeAction::ResizePaneRight,
    ),
    (
        KeyChordSpec {
            code: KeyCodeSpec::Right,
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

#[nutype(
    const_fn,
    validate(greater_or_equal = 32, less_or_equal = 126),
    derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)
)]
struct AsciiChar(u8);

impl AsciiChar {
    fn from_client_character(character: char) -> Option<Self> {
        if !character.is_ascii() {
            return None;
        }
        Self::try_new(character as u8).ok()
    }

    const fn byte(self) -> u8 {
        self.into_inner()
    }

    fn canonical(self, modifiers: KeyModifiers) -> Option<Self> {
        Self::try_new(modifiers.canonical_byte(self.byte())).ok()
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
    fn canonical(self, modifiers: KeyModifiers) -> Option<Self> {
        match self {
            Self::Char(character) => Some(Self::Char(character.canonical(modifiers)?)),
            Self::Down => Some(Self::Down),
            Self::Esc => Some(Self::Esc),
            Self::Left => Some(Self::Left),
            Self::Right => Some(Self::Right),
            Self::Up => Some(Self::Up),
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

    const fn canonical_byte(self, byte: u8) -> u8 {
        match self {
            Self::None => byte,
            Self::Alt | Self::Ctrl | Self::CtrlAlt => byte.to_ascii_lowercase(),
            Self::Shift | Self::ShiftAlt | Self::CtrlShift | Self::CtrlAltShift => match byte {
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
            .to_ascii_uppercase(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum KeyCodeSpec {
    Char(u8),
    Down,
    Esc,
    Left,
    Right,
    Up,
}

impl KeyCodeSpec {
    const fn from_character(character: char) -> Self {
        assert!(character.is_ascii() && !character.is_ascii_control());
        Self::Char(character as u8)
    }

    const fn canonical(self, modifiers: KeyModifiers) -> Self {
        match self {
            Self::Char(byte) => Self::Char(modifiers.canonical_byte(byte)),
            Self::Down => Self::Down,
            Self::Esc => Self::Esc,
            Self::Left => Self::Left,
            Self::Right => Self::Right,
            Self::Up => Self::Up,
        }
    }

    fn compile(self) -> rootcause::Result<SupportedKeyCode> {
        match self {
            Self::Char(byte) => Ok(SupportedKeyCode::Char(AsciiChar::try_new(byte)?)),
            Self::Down => Ok(SupportedKeyCode::Down),
            Self::Esc => Ok(SupportedKeyCode::Esc),
            Self::Left => Ok(SupportedKeyCode::Left),
            Self::Right => Ok(SupportedKeyCode::Right),
            Self::Up => Ok(SupportedKeyCode::Up),
        }
    }
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
    fn from_client_key(key: &ClientKey) -> Option<Self> {
        let code = match key.code {
            ClientKeyCode::Char(character) => {
                let character = AsciiChar::from_client_character(character)?;
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
            code: code.canonical(modifiers)?,
            modifiers,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct KeyChordSpec {
    code: KeyCodeSpec,
    modifiers: KeyModifiers,
}

impl KeyChordSpec {
    fn compile(self) -> rootcause::Result<KeyChord> {
        Ok(KeyChord {
            code: self.code.compile()?,
            modifiers: self.modifiers,
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
            (KeyCodeSpec::Char(left), KeyCodeSpec::Char(right)) => left == right,
            (KeyCodeSpec::Down, KeyCodeSpec::Down)
            | (KeyCodeSpec::Esc, KeyCodeSpec::Esc)
            | (KeyCodeSpec::Left, KeyCodeSpec::Left)
            | (KeyCodeSpec::Right, KeyCodeSpec::Right)
            | (KeyCodeSpec::Up, KeyCodeSpec::Up) => true,
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
                KeyCodeSpec::Char(_)
                | KeyCodeSpec::Down
                | KeyCodeSpec::Esc
                | KeyCodeSpec::Left
                | KeyCodeSpec::Right
                | KeyCodeSpec::Up,
                KeyModifiers::None,
            )
            | (KeyCodeSpec::Char(b'n' | b'p'), KeyModifiers::CtrlAlt) => KeyChordValidation::Supported,
            (KeyCodeSpec::Char(character), KeyModifiers::Alt) => match legacy_character_support(character) {
                LegacyCharacterSupport::Alt | LegacyCharacterSupport::AltAndShiftAlt => KeyChordValidation::Supported,
                LegacyCharacterSupport::ShiftAlt | LegacyCharacterSupport::Unsupported => {
                    KeyChordValidation::Unsupported
                }
            },
            (KeyCodeSpec::Char(character), KeyModifiers::ShiftAlt) => match legacy_character_support(character) {
                LegacyCharacterSupport::ShiftAlt | LegacyCharacterSupport::AltAndShiftAlt => {
                    KeyChordValidation::Supported
                }
                LegacyCharacterSupport::Alt | LegacyCharacterSupport::Unsupported => KeyChordValidation::Unsupported,
            },
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

const fn assert_unique_keybindings<Action>(bindings: &[(KeyChordSpec, Action)]) {
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
    left: &[(KeyChordSpec, LeftAction)],
    right: &[(KeyChordSpec, RightAction)],
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

impl<Action> Keymap<Action> {
    fn new<const LENGTH: usize>(bindings: [(KeyChordSpec, Action); LENGTH]) -> rootcause::Result<Self> {
        let bindings = bindings
            .into_iter()
            .map(|(chord, action)| Ok((chord.compile()?, action)))
            .collect::<rootcause::Result<BTreeMap<_, _>>>()?;
        Ok(Self { bindings })
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

impl KeybindingsConfig {
    /// Compile the static keybinding inventory into validated runtime keys.
    ///
    /// # Errors
    /// Returns an error if a configured character is not printable ASCII.
    pub fn new() -> rootcause::Result<Self> {
        Ok(Self {
            local: Keymap::new(DEFAULT_LOCAL_KEYBINDINGS)?,
            normal: Keymap::new(DEFAULT_NORMAL_KEYBINDINGS)?,
            resize: Keymap::new(DEFAULT_RESIZE_KEYBINDINGS)?,
        })
    }

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

    #[rstest::rstest]
    #[case::control(31)]
    #[case::del(127)]
    #[case::non_ascii(128)]
    fn test_key_code_spec_compile_when_character_is_invalid_returns_error(#[case] byte: u8) {
        assert_that!(KeyCodeSpec::Char(byte).compile(), err(anything()));
    }

    #[test]
    fn test_keybindings_new_when_contains_unique_inventory_returns_config() {
        let keybindings = KeybindingsConfig::new().unwrap();

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
        let keybindings = KeybindingsConfig::new().unwrap();
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
        let keybindings = KeybindingsConfig::new().unwrap();
        let key = ClientKey {
            code,
            modifiers,
            raw_bytes: Vec::new(),
        };

        assert_that!(keybindings.resolve(mode, &key), some(eq(action)));
    }

    #[test]
    fn test_keybindings_resolve_when_unsupported_code_arrives_returns_none() {
        let keybindings = KeybindingsConfig::new().unwrap();
        let key = ClientKey {
            code: ClientKeyCode::Enter,
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: Vec::new(),
        };

        assert_that!(keybindings.resolve(KeybindingMode::Normal, &key), none());
    }

    #[test]
    fn test_keybindings_resolve_when_shifted_punctuation_arrives_returns_tab_action() {
        let keybindings = KeybindingsConfig::new().unwrap();
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
        let lower = KeyChordSpec {
            code: KeyCodeSpec::from_character('c'),
            modifiers: KeyModifiers::SHIFT_ALT,
        };
        let upper = KeyChordSpec {
            code: KeyCodeSpec::from_character('C'),
            modifiers: KeyModifiers::SHIFT_ALT,
        };

        assert_that!(lower.canonical_comparison(upper), eq(KeyChordComparison::Same));
        assert_that!(lower.validation(), eq(KeyChordValidation::NonCanonical));
        assert_that!(upper.validation(), eq(KeyChordValidation::Supported));
    }

    #[rstest::rstest]
    #[case::alt_left_bracket(KeyChordSpec { code: KeyCodeSpec::from_character('['), modifiers: KeyModifiers::Alt })]
    #[case::alt_right_bracket(KeyChordSpec { code: KeyCodeSpec::from_character(']'), modifiers: KeyModifiers::Alt })]
    #[case::alt_shifted_punctuation(KeyChordSpec { code: KeyCodeSpec::from_character('!'), modifiers: KeyModifiers::Alt })]
    #[case::ctrl_alt_other_character(KeyChordSpec { code: KeyCodeSpec::from_character('a'), modifiers: KeyModifiers::CtrlAlt })]
    #[case::shifted_arrow(KeyChordSpec { code: KeyCodeSpec::Left, modifiers: KeyModifiers::Shift })]
    fn test_key_chord_when_legacy_decoder_cannot_emit_chord_reports_unsupported(#[case] chord: KeyChordSpec) {
        assert_that!(chord.validation(), eq(KeyChordValidation::Unsupported));
    }
}
