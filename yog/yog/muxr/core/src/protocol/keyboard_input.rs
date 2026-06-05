use serde::Serialize;

/// Normalized key code carried with the original terminal bytes.
#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum ClientKeyCode {
    Backspace,
    Char(char),
    Down,
    Enter,
    Esc,
    Left,
    Right,
    Tab,
    Unknown,
    Up,
}

/// Keyboard modifiers observed by the muxr client.
#[derive(rkyv::Archive, Clone, Copy, Debug, Default, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct ClientKeyModifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl ClientKeyModifiers {
    pub const ALT: Self = Self {
        alt: true,
        ctrl: false,
        shift: false,
    };
    pub const CTRL_ALT: Self = Self {
        alt: true,
        ctrl: true,
        shift: false,
    };
    pub const NONE: Self = Self {
        alt: false,
        ctrl: false,
        shift: false,
    };
    pub const SHIFT_ALT: Self = Self {
        alt: true,
        ctrl: false,
        shift: true,
    };
}

/// One ordered keyboard event from the muxr client.
#[derive(rkyv::Archive, Clone, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub struct ClientKey {
    pub code: ClientKeyCode,
    pub modifiers: ClientKeyModifiers,
    pub raw_bytes: Vec<u8>,
}
