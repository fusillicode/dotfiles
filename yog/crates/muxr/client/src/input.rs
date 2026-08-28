use muxr_config::KeybindingMode;
use muxr_config::KeybindingsConfig;
use muxr_core::ClientKey;
use muxr_core::ClientKeyCode;
use muxr_core::ClientKeyModifiers;
use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::ClientMousePosition;

const CTRL_N: u8 = 0x0e;
const CTRL_P: u8 = 0x10;
const ESC: u8 = 0x1b;
const MAX_PENDING_ESCAPE_BYTES: usize = 64;
const MAX_PENDING_CONTROL_STRING_BYTES: usize = 4096;
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedInput {
    Input(Vec<u8>),
    Key(ClientKey),
    Mouse(ClientMouseEvent),
    Paste(Vec<u8>),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum PendingInput {
    #[default]
    None,
    EscapeSequence(Vec<u8>),
    AmbiguousControlString(Vec<u8>),
    ControlString {
        bytes: Vec<u8>,
        kind: ControlStringKind,
    },
    Paste(Vec<u8>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlStringKind {
    Osc,
    Other,
}

impl ControlStringKind {
    const fn from_prefix(byte: u8) -> Option<Self> {
        match byte {
            b']' => Some(Self::Osc),
            b'P' | b'X' | b'^' | b'_' => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlStringStatus {
    Complete,
    Incomplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyAltCharacter {
    Shifted(char),
    Unshifted(char),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SgrMouseEvent {
    Event(ClientMouseEvent),
    Ignored,
}

impl SgrMouseEvent {
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.first() != Some(&ESC) || bytes.get(1) != Some(&b'[') || bytes.get(2) != Some(&b'<') {
            return None;
        }
        let release = match bytes.last() {
            Some(b'M') => false,
            Some(b'm') => true,
            Some(_) | None => return Some(Self::Ignored),
        };
        let phase = if release {
            ClientMouseEventPhase::Release
        } else {
            ClientMouseEventPhase::Press
        };
        let Some((button, position)) = self::sgr_mouse_button_and_position(bytes) else {
            return Some(Self::Ignored);
        };
        Some(Self::Event(ClientMouseEvent {
            button,
            phase,
            position,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KittyKeyModifiers {
    Supported(ClientKeyModifiers),
    Unsupported,
}

impl KittyKeyModifiers {
    fn from_raw(raw: &[u8]) -> Option<Self> {
        let flags = self::parse_mouse_number(raw)?.checked_sub(1)?;
        if flags & !0b111 != 0 {
            return Some(Self::Unsupported);
        }
        Some(Self::Supported(ClientKeyModifiers {
            alt: flags & 0b010 != 0,
            ctrl: flags & 0b100 != 0,
            shift: flags & 0b001 != 0,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputDecoder {
    pending: PendingInput,
    keybindings: KeybindingsConfig,
}

impl Default for InputDecoder {
    fn default() -> Self {
        Self::with_keybindings(KeybindingsConfig::default())
    }
}

impl InputDecoder {
    pub(crate) const fn with_keybindings(keybindings: KeybindingsConfig) -> Self {
        Self {
            pending: PendingInput::None,
            keybindings,
        }
    }

    #[must_use]
    pub fn decode(&mut self, bytes: &[u8]) -> Vec<DecodedInput> {
        let mut decoded = Vec::new();
        let mut input = Vec::new();

        for byte in bytes {
            self.push_byte(*byte, &mut input, &mut decoded);
        }

        self::push_input(&mut decoded, &mut input);
        decoded
    }

    #[must_use]
    pub fn finalize(&mut self) -> Vec<DecodedInput> {
        let mut decoded = Vec::new();
        let mut input = Vec::new();

        self::finalize_pending_input(self, &mut input, &mut decoded);

        self::push_input(&mut decoded, &mut input);
        decoded
    }

    #[must_use]
    pub const fn idle_timeout(&self) -> InputIdleTimeout {
        match self.pending {
            PendingInput::EscapeSequence(_) | PendingInput::AmbiguousControlString(_) => InputIdleTimeout::Needed,
            PendingInput::None | PendingInput::ControlString { .. } | PendingInput::Paste(_) => {
                InputIdleTimeout::NotNeeded
            }
        }
    }

    fn push_byte(&mut self, byte: u8, input: &mut Vec<u8>, decoded: &mut Vec<DecodedInput>) {
        match std::mem::take(&mut self.pending) {
            PendingInput::None => {
                if byte == ESC {
                    self.pending = PendingInput::EscapeSequence(vec![ESC]);
                } else if let Some(key) = self::key_for_plain_byte(byte) {
                    self::push_key(decoded, input, key);
                } else {
                    input.push(byte);
                }
            }
            PendingInput::EscapeSequence(mut bytes) => {
                bytes.push(byte);
                if bytes.len() == 2
                    && let Some(kind) = ControlStringKind::from_prefix(byte)
                {
                    self.pending = if self::key_for_escaped_byte(byte, &self.keybindings).is_some() {
                        PendingInput::AmbiguousControlString(bytes)
                    } else {
                        PendingInput::ControlString { bytes, kind }
                    };
                } else if PendingEscapeStatus::from(bytes.as_slice()) == PendingEscapeStatus::Incomplete {
                    self.pending = PendingInput::EscapeSequence(bytes);
                } else if bytes == BRACKETED_PASTE_START {
                    self::push_input(decoded, input);
                    self.pending = PendingInput::Paste(Vec::new());
                } else {
                    self::finish_escape_sequence(bytes, &self.keybindings, input, decoded);
                }
            }
            PendingInput::AmbiguousControlString(mut bytes) => {
                bytes.push(byte);
                if let Some(&prefix) = bytes.get(1)
                    && let Some(kind) = ControlStringKind::from_prefix(prefix)
                {
                    match self::control_string_status(&bytes, kind) {
                        ControlStringStatus::Complete => input.extend(bytes),
                        ControlStringStatus::Incomplete if bytes.len() >= MAX_PENDING_CONTROL_STRING_BYTES => {
                            self::flush_control_string(bytes, input, decoded);
                            self.pending = PendingInput::ControlString {
                                bytes: Vec::new(),
                                kind,
                            };
                        }
                        ControlStringStatus::Incomplete => {
                            self.pending = PendingInput::AmbiguousControlString(bytes);
                        }
                    }
                } else {
                    self.pending = PendingInput::AmbiguousControlString(bytes);
                }
            }
            PendingInput::ControlString { mut bytes, kind } => {
                bytes.push(byte);
                match self::control_string_status(&bytes, kind) {
                    ControlStringStatus::Complete => input.extend(bytes),
                    ControlStringStatus::Incomplete if bytes.len() >= MAX_PENDING_CONTROL_STRING_BYTES => {
                        self::flush_control_string(bytes, input, decoded);
                        self.pending = PendingInput::ControlString {
                            bytes: Vec::new(),
                            kind,
                        };
                    }
                    ControlStringStatus::Incomplete => {
                        self.pending = PendingInput::ControlString { bytes, kind };
                    }
                }
            }
            PendingInput::Paste(mut bytes) => {
                bytes.push(byte);
                if bytes.ends_with(BRACKETED_PASTE_END) {
                    let paste_len = bytes.len().saturating_sub(BRACKETED_PASTE_END.len());
                    bytes.truncate(paste_len);
                    decoded.push(DecodedInput::Paste(bytes));
                } else {
                    self.pending = PendingInput::Paste(bytes);
                }
            }
        }
    }
}

fn finish_ambiguous_control_string(
    decoder: &mut InputDecoder,
    bytes: Vec<u8>,
    input: &mut Vec<u8>,
    events: &mut Vec<DecodedInput>,
) {
    let Some(&byte) = bytes.get(1) else {
        input.extend(bytes);
        return;
    };
    let Some(rest) = bytes.get(2..) else {
        input.extend(bytes);
        return;
    };
    let Some(key) = self::key_for_escaped_byte(byte, &decoder.keybindings) else {
        input.extend(bytes);
        return;
    };

    self::push_key(events, input, key);
    for byte in rest {
        decoder.push_byte(*byte, input, events);
    }
}

fn finalize_pending_input(decoder: &mut InputDecoder, input: &mut Vec<u8>, events: &mut Vec<DecodedInput>) {
    loop {
        match std::mem::take(&mut decoder.pending) {
            PendingInput::None => return,
            PendingInput::EscapeSequence(bytes) if bytes.as_slice() == [ESC] => {
                self::push_key(
                    events,
                    input,
                    self::key(ClientKeyCode::Esc, ClientKeyModifiers::NONE, &bytes),
                );
                return;
            }
            PendingInput::AmbiguousControlString(bytes) => {
                self::finish_ambiguous_control_string(decoder, bytes, input, events);
            }
            PendingInput::EscapeSequence(bytes) | PendingInput::ControlString { bytes, .. } => {
                input.extend(bytes);
                return;
            }
            PendingInput::Paste(bytes) => {
                input.extend(BRACKETED_PASTE_START);
                input.extend(bytes);
                return;
            }
        }
    }
}

fn finish_escape_sequence(
    bytes: Vec<u8>,
    keybindings: &KeybindingsConfig,
    input: &mut Vec<u8>,
    decoded: &mut Vec<DecodedInput>,
) {
    if let [ESC, byte] = bytes.as_slice()
        && let Some(key) = self::key_for_escaped_byte(*byte, keybindings)
    {
        self::push_key(decoded, input, key);
        return;
    }

    if let Some(key) = self::key_for_csi_sequence(&bytes) {
        self::push_key(decoded, input, key);
        return;
    }

    if let Some(event) = SgrMouseEvent::from_bytes(&bytes) {
        self::push_input(decoded, input);
        match event {
            SgrMouseEvent::Ignored => {}
            SgrMouseEvent::Event(event) => decoded.push(DecodedInput::Mouse(event)),
        }
        return;
    }

    input.extend(bytes);
}

fn key_for_plain_byte(byte: u8) -> Option<ClientKey> {
    (byte.is_ascii() && !byte.is_ascii_control())
        .then(|| self::key(ClientKeyCode::Char(char::from(byte)), ClientKeyModifiers::NONE, &[byte]))
}

fn key_for_escaped_byte(byte: u8, keybindings: &KeybindingsConfig) -> Option<ClientKey> {
    let (code, modifiers) = match byte {
        CTRL_N => (ClientKeyCode::Char('n'), ClientKeyModifiers::CTRL_ALT),
        CTRL_P => (ClientKeyCode::Char('p'), ClientKeyModifiers::CTRL_ALT),
        _ => match self::legacy_alt_character(byte)? {
            LegacyAltCharacter::Shifted(character) => (ClientKeyCode::Char(character), ClientKeyModifiers::SHIFT_ALT),
            LegacyAltCharacter::Unshifted(character) => (ClientKeyCode::Char(character), ClientKeyModifiers::ALT),
        },
    };

    if byte == b']' {
        return None;
    }

    let key = self::key(code, modifiers, &[ESC, byte]);
    (keybindings.resolve_local(&key).is_some()
        || keybindings.resolve(KeybindingMode::Normal, &key).is_some()
        || keybindings.resolve(KeybindingMode::Resize, &key).is_some())
    .then_some(key)
}

fn control_string_status(bytes: &[u8], kind: ControlStringKind) -> ControlStringStatus {
    if bytes.ends_with(b"\x1b\\") || (kind == ControlStringKind::Osc && bytes.last() == Some(&b'\x07')) {
        ControlStringStatus::Complete
    } else {
        ControlStringStatus::Incomplete
    }
}

fn flush_control_string(bytes: Vec<u8>, input: &mut Vec<u8>, decoded: &mut Vec<DecodedInput>) {
    self::push_input(decoded, input);
    decoded.push(DecodedInput::Input(bytes));
}

fn legacy_alt_character(byte: u8) -> Option<LegacyAltCharacter> {
    let shifted_character = match byte {
        b'!' => Some(LegacyAltCharacter::Shifted('1')),
        b'@' => Some(LegacyAltCharacter::Shifted('2')),
        b'#' => Some(LegacyAltCharacter::Shifted('3')),
        b'$' => Some(LegacyAltCharacter::Shifted('4')),
        b'%' => Some(LegacyAltCharacter::Shifted('5')),
        b'^' => Some(LegacyAltCharacter::Shifted('6')),
        b'&' => Some(LegacyAltCharacter::Shifted('7')),
        b'*' => Some(LegacyAltCharacter::Shifted('8')),
        b'(' => Some(LegacyAltCharacter::Shifted('9')),
        b')' => Some(LegacyAltCharacter::Shifted('0')),
        b'_' => Some(LegacyAltCharacter::Shifted('-')),
        b'+' => Some(LegacyAltCharacter::Shifted('=')),
        b'{' => Some(LegacyAltCharacter::Shifted('[')),
        b'}' => Some(LegacyAltCharacter::Shifted(']')),
        b'|' => Some(LegacyAltCharacter::Shifted('\\')),
        b':' => Some(LegacyAltCharacter::Shifted(';')),
        b'"' => Some(LegacyAltCharacter::Shifted('\'')),
        b'<' => Some(LegacyAltCharacter::Shifted(',')),
        b'>' => Some(LegacyAltCharacter::Shifted('.')),
        b'?' => Some(LegacyAltCharacter::Shifted('/')),
        b'~' => Some(LegacyAltCharacter::Shifted('`')),
        _ => None,
    };
    if shifted_character.is_some() {
        return shifted_character;
    }
    if byte.is_ascii_graphic() || byte == b' ' {
        let character = char::from(byte);
        return Some(if character.is_ascii_uppercase() {
            LegacyAltCharacter::Shifted(character)
        } else {
            LegacyAltCharacter::Unshifted(character)
        });
    }
    None
}

fn key_for_csi_sequence(bytes: &[u8]) -> Option<ClientKey> {
    if let Some(key) = self::key_for_kitty_keyboard_sequence(bytes) {
        return Some(key);
    }

    let [ESC, b'[', byte] = bytes else {
        return None;
    };

    match byte {
        b'A' => Some(self::key(ClientKeyCode::Up, ClientKeyModifiers::NONE, bytes)),
        b'B' => Some(self::key(ClientKeyCode::Down, ClientKeyModifiers::NONE, bytes)),
        b'C' => Some(self::key(ClientKeyCode::Right, ClientKeyModifiers::NONE, bytes)),
        b'D' => Some(self::key(ClientKeyCode::Left, ClientKeyModifiers::NONE, bytes)),
        _ => None,
    }
}

fn key_for_kitty_keyboard_sequence(bytes: &[u8]) -> Option<ClientKey> {
    if bytes.first() != Some(&ESC) || bytes.get(1) != Some(&b'[') || bytes.last() != Some(&b'u') {
        return None;
    }

    let body_end = bytes.len().checked_sub(1)?;
    let body = bytes.get(2..body_end)?;
    let mut parts = body.split(|byte| *byte == b';');
    let key_number = parts.next().and_then(self::parse_mouse_number)?;
    let modifiers = match parts.next() {
        Some(raw) => KittyKeyModifiers::from_raw(raw)?,
        None => KittyKeyModifiers::Supported(ClientKeyModifiers::NONE),
    };
    if parts.next().is_some() {
        return None;
    }
    let KittyKeyModifiers::Supported(modifiers) = modifiers else {
        // The wire type cannot represent kitty's higher modifier bits. Preserve the raw sequence as an unknown key so
        // muxr shortcuts do not accidentally fire after dropping unsupported bits.
        return Some(self::key(ClientKeyCode::Unknown, ClientKeyModifiers::NONE, bytes));
    };

    let code = match key_number {
        9 => ClientKeyCode::Tab,
        13 => ClientKeyCode::Enter,
        27 => ClientKeyCode::Esc,
        127 => ClientKeyCode::Backspace,
        32..=126 => ClientKeyCode::Char(self::kitty_ascii_character(key_number, modifiers)?),
        _ => ClientKeyCode::Unknown,
    };

    Some(self::key(code, modifiers, bytes))
}

fn kitty_ascii_character(key_number: u16, modifiers: ClientKeyModifiers) -> Option<char> {
    let character = char::from(u8::try_from(key_number).ok()?);
    // Kitty level 1 may report a base lowercase ASCII key plus the Shift flag. Muxr bindings historically match the
    // shifted legacy byte, such as Alt-Shift-V -> Char('V'), so normalize letters before server shortcut resolution.
    if modifiers.shift && character.is_ascii_lowercase() {
        Some(character.to_ascii_uppercase())
    } else {
        Some(character)
    }
}

fn sgr_mouse_button_and_position(bytes: &[u8]) -> Option<(u16, ClientMousePosition)> {
    let body_end = bytes.len().checked_sub(1)?;
    let body = bytes.get(3..body_end)?;
    let mut parts = body.split(|byte| *byte == b';');
    let button = parts.next().and_then(self::parse_mouse_number)?;
    let col = parts
        .next()
        .and_then(self::parse_mouse_number)
        .and_then(|col| col.checked_sub(1))?;
    let row = parts
        .next()
        .and_then(self::parse_mouse_number)
        .and_then(|row| row.checked_sub(1))?;
    if parts.next().is_some() {
        return None;
    }

    Some((button, ClientMousePosition { row, col }))
}

fn parse_mouse_number(raw: &[u8]) -> Option<u16> {
    if raw.is_empty() {
        return None;
    }

    let mut value = 0_u16;
    for byte in raw {
        if !byte.is_ascii_digit() {
            return None;
        }
        let digit = u16::from(byte.saturating_sub(b'0'));
        value = value.checked_mul(10)?.checked_add(digit)?;
    }
    Some(value)
}

fn push_key(decoded: &mut Vec<DecodedInput>, input: &mut Vec<u8>, key: ClientKey) {
    self::push_input(decoded, input);
    decoded.push(DecodedInput::Key(key));
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputIdleTimeout {
    Needed,
    NotNeeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingEscapeStatus {
    Complete,
    Incomplete,
}

impl From<&[u8]> for PendingEscapeStatus {
    fn from(bytes: &[u8]) -> Self {
        if bytes.len() > MAX_PENDING_ESCAPE_BYTES {
            return Self::Complete;
        }

        let complete = match bytes {
            [ESC] | [ESC, b'['] | [ESC, b'[', b'<'] => return Self::Incomplete,
            [ESC, b'[', rest @ ..] => rest.last().is_some_and(|byte| (0x40..=0x7e).contains(byte)),
            _ => true,
        };
        if complete { Self::Complete } else { Self::Incomplete }
    }
}

fn push_input(decoded: &mut Vec<DecodedInput>, input: &mut Vec<u8>) {
    if input.is_empty() {
        return;
    }

    decoded.push(DecodedInput::Input(std::mem::take(input)));
}

fn key(code: ClientKeyCode, modifiers: ClientKeyModifiers, raw_bytes: &[u8]) -> ClientKey {
    ClientKey {
        code,
        modifiers,
        raw_bytes: raw_bytes.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_input_decoder_decode_when_printable_bytes_are_plain_returns_keys() {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(b"abc"),
            eq(vec![
                DecodedInput::Key(key(ClientKeyCode::Char('a'), ClientKeyModifiers::NONE, b"a")),
                DecodedInput::Key(key(ClientKeyCode::Char('b'), ClientKeyModifiers::NONE, b"b")),
                DecodedInput::Key(key(ClientKeyCode::Char('c'), ClientKeyModifiers::NONE, b"c")),
            ])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_bare_enter_arrives_preserves_input_bytes() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\r"), eq(vec![DecodedInput::Input(b"\r".to_vec())]));
    }

    #[rstest]
    #[case::create_tab(b"\x1bE", ClientKeyCode::Char('E'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::focus_previous_tab(b"\x1bP", ClientKeyCode::Char('P'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::focus_next_tab(b"\x1bN", ClientKeyCode::Char('N'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::move_tab_previous(b"\x1b\x10", ClientKeyCode::Char('p'), ClientKeyModifiers::CTRL_ALT)]
    #[case::move_tab_next(b"\x1b\x0e", ClientKeyCode::Char('n'), ClientKeyModifiers::CTRL_ALT)]
    #[case::focus_pane_left(b"\x1bH", ClientKeyCode::Char('H'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::focus_pane_down(b"\x1bJ", ClientKeyCode::Char('J'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::focus_pane_up(b"\x1bK", ClientKeyCode::Char('K'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::focus_pane_right(b"\x1bL", ClientKeyCode::Char('L'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::split_pane_vertical(b"\x1bV", ClientKeyCode::Char('V'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::split_pane_horizontal(b"\x1bD", ClientKeyCode::Char('D'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::toggle_pane_fullscreen(b"\x1bF", ClientKeyCode::Char('F'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::close_pane(b"\x1bW", ClientKeyCode::Char('W'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::enter_resize_mode(b"\x1bR", ClientKeyCode::Char('R'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::open_scrollback_editor(b"\x1bS", ClientKeyCode::Char('S'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::kitty_create_tab(b"\x1b[101;4u", ClientKeyCode::Char('E'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::kitty_split_pane_vertical(b"\x1b[118;4u", ClientKeyCode::Char('V'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::kitty_move_tab_previous(b"\x1b[112;7u", ClientKeyCode::Char('p'), ClientKeyModifiers::CTRL_ALT)]
    fn test_input_decoder_decode_when_shortcut_arrives_returns_key(
        #[case] bytes: &[u8],
        #[case] code: ClientKeyCode,
        #[case] modifiers: ClientKeyModifiers,
    ) {
        let mut decoder = InputDecoder::default();

        assert_that!(
            self::decode_and_finalize(&mut decoder, bytes),
            eq(vec![DecodedInput::Key(key(code, modifiers, bytes))])
        );
    }

    #[rstest]
    #[case::legacy_copy(b"\x1bC", 'C')]
    #[case::kitty_copy(b"\x1b[99;4u", 'C')]
    #[case::legacy_inline_copy(b"\x1bX", 'X')]
    #[case::kitty_inline_copy(b"\x1b[120;4u", 'X')]
    fn test_input_decoder_decode_when_local_shortcut_arrives_returns_key(
        #[case] bytes: &[u8],
        #[case] character: char,
    ) {
        let mut decoder = InputDecoder::default();

        assert_that!(
            self::decode_and_finalize(&mut decoder, bytes),
            eq(vec![DecodedInput::Key(key(
                ClientKeyCode::Char(character),
                ClientKeyModifiers::SHIFT_ALT,
                bytes,
            ))])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_shortcut_is_between_input_splits_actions() {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(b"a\x1bEb"),
            eq(vec![
                DecodedInput::Key(key(ClientKeyCode::Char('a'), ClientKeyModifiers::NONE, b"a")),
                DecodedInput::Key(key(ClientKeyCode::Char('E'), ClientKeyModifiers::SHIFT_ALT, b"\x1bE",)),
                DecodedInput::Key(key(ClientKeyCode::Char('b'), ClientKeyModifiers::NONE, b"b")),
            ])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_unknown_legacy_alt_key_arrives_preserves_input_bytes() {
        let mut decoder = InputDecoder::default();
        let bytes = b"\x1bY";

        assert_that!(decoder.decode(bytes), eq(vec![DecodedInput::Input(bytes.to_vec())]));
    }

    #[rstest]
    #[case::bel_terminated(b"\x1b]0;title\x07")]
    #[case::st_terminated(b"\x1b]0;title\x1b\\")]
    #[case::contains_muxr_prefix(b"\x1b]0;\x1bC\x1b\\")]
    fn test_input_decoder_decode_when_osc_arrives_preserves_control_string_bytes(#[case] bytes: &[u8]) {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(bytes), eq(vec![DecodedInput::Input(bytes.to_vec())]));
    }

    #[rstest::rstest]
    #[case::dcs(b"\x1bP1;2\x1b\\")]
    #[case::sos(b"\x1bX1;2\x1b\\")]
    #[case::pm(b"\x1b^1;2\x1b\\")]
    fn test_input_decoder_decode_when_legacy_shortcut_prefix_is_control_string_preserves_bytes(#[case] bytes: &[u8]) {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(bytes), eq(vec![DecodedInput::Input(bytes.to_vec())]));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[test]
    fn test_input_decoder_finalize_when_ambiguous_legacy_shortcut_arrives_returns_key() {
        let mut decoder = InputDecoder::default();
        let bytes = b"\x1bP";

        assert_that!(decoder.decode(bytes), eq(Vec::<DecodedInput>::new()));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::Needed));
        assert_that!(
            decoder.finalize(),
            eq(vec![DecodedInput::Key(key(
                ClientKeyCode::Char('P'),
                ClientKeyModifiers::SHIFT_ALT,
                bytes,
            ))])
        );
    }

    #[test]
    fn test_input_decoder_finalize_when_ambiguous_legacy_shortcut_has_suffix_replays_suffix() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\x1bPa"), eq(Vec::<DecodedInput>::new()));
        assert_that!(
            decoder.finalize(),
            eq(vec![
                DecodedInput::Key(key(ClientKeyCode::Char('P'), ClientKeyModifiers::SHIFT_ALT, b"\x1bP")),
                DecodedInput::Key(key(ClientKeyCode::Char('a'), ClientKeyModifiers::NONE, b"a")),
            ])
        );
    }

    #[test]
    fn test_input_decoder_finalize_when_ambiguous_suffix_ends_in_escape_drains_pending_key() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\x1bP\x1b"), eq(Vec::<DecodedInput>::new()));
        assert_that!(
            decoder.finalize(),
            eq(vec![
                DecodedInput::Key(key(ClientKeyCode::Char('P'), ClientKeyModifiers::SHIFT_ALT, b"\x1bP")),
                DecodedInput::Key(key(ClientKeyCode::Esc, ClientKeyModifiers::NONE, b"\x1b")),
            ])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_control_string_exceeds_buffer_limit_flushes_raw_chunks() {
        let mut decoder = InputDecoder::default();
        let mut bytes = vec![ESC, b']'];
        bytes.extend(std::iter::repeat_n(b'a', MAX_PENDING_CONTROL_STRING_BYTES));

        let mut events = decoder.decode(&bytes);
        assert_that!(events.len(), eq(1));
        assert_that!(
            events.first(),
            some(eq(&DecodedInput::Input(
                bytes[..MAX_PENDING_CONTROL_STRING_BYTES].to_vec()
            )))
        );
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
        events.extend(decoder.finalize());
        assert_that!(
            events.iter().all(|event| matches!(event, DecodedInput::Input(_))),
            eq(true)
        );
        let preserved = events
            .into_iter()
            .flat_map(|decoded| match decoded {
                DecodedInput::Input(bytes) => bytes,
                DecodedInput::Key(_) | DecodedInput::Mouse(_) | DecodedInput::Paste(_) => Vec::new(),
            })
            .collect::<Vec<_>>();
        assert_that!(preserved, eq(bytes));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[test]
    fn test_input_decoder_decode_when_flushed_osc_reaches_bel_terminator_returns_following_key() {
        let mut decoder = InputDecoder::default();
        let mut bytes = vec![ESC, b']'];
        bytes.extend(std::iter::repeat_n(b'a', MAX_PENDING_CONTROL_STRING_BYTES));
        bytes.extend(*b"\x07z");

        assert_that!(
            decoder.decode(&bytes),
            eq(vec![
                DecodedInput::Input(bytes[..MAX_PENDING_CONTROL_STRING_BYTES].to_vec()),
                DecodedInput::Input(
                    bytes[MAX_PENDING_CONTROL_STRING_BYTES..MAX_PENDING_CONTROL_STRING_BYTES + 3].to_vec()
                ),
                DecodedInput::Key(key(ClientKeyCode::Char('z'), ClientKeyModifiers::NONE, b"z")),
            ])
        );
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[test]
    fn test_input_decoder_decode_when_unknown_csi_arrives_preserves_bytes() {
        let mut decoder = InputDecoder::default();
        let bytes = b"\x1b[1~";

        assert_that!(decoder.decode(bytes), eq(vec![DecodedInput::Input(bytes.to_vec())]));
    }

    #[test]
    fn test_input_decoder_decode_when_shortcut_is_split_preserves_pending_prefix() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\x1b"), eq(Vec::<DecodedInput>::new()));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::Needed));
        assert_that!(
            decoder.decode(b"E"),
            eq(vec![DecodedInput::Key(key(
                ClientKeyCode::Char('E'),
                ClientKeyModifiers::SHIFT_ALT,
                b"\x1bE",
            ))])
        );
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[test]
    fn test_input_decoder_finalize_when_bare_escape_arrives_returns_key() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\x1b"), eq(Vec::<DecodedInput>::new()));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::Needed));
        assert_that!(
            decoder.finalize(),
            eq(vec![DecodedInput::Key(key(
                ClientKeyCode::Esc,
                ClientKeyModifiers::NONE,
                b"\x1b",
            ))])
        );
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[test]
    fn test_input_decoder_finalize_when_pending_unknown_sequence_arrives_preserves_bytes() {
        let mut decoder = InputDecoder::default();
        let bytes = b"\x1b[1";

        assert_that!(decoder.decode(bytes), eq(Vec::<DecodedInput>::new()));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::Needed));
        assert_that!(decoder.finalize(), eq(vec![DecodedInput::Input(bytes.to_vec())]));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[rstest]
    #[case::left(b"h", ClientKeyCode::Char('h'))]
    #[case::down(b"j", ClientKeyCode::Char('j'))]
    #[case::up(b"k", ClientKeyCode::Char('k'))]
    #[case::right(b"l", ClientKeyCode::Char('l'))]
    #[case::arrow_left(b"\x1b[D", ClientKeyCode::Left)]
    #[case::arrow_down(b"\x1b[B", ClientKeyCode::Down)]
    #[case::arrow_up(b"\x1b[A", ClientKeyCode::Up)]
    #[case::arrow_right(b"\x1b[C", ClientKeyCode::Right)]
    fn test_input_decoder_decode_when_server_mode_key_arrives_returns_key(
        #[case] bytes: &[u8],
        #[case] code: ClientKeyCode,
    ) {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(bytes),
            eq(vec![DecodedInput::Key(key(code, ClientKeyModifiers::NONE, bytes))])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_arrow_is_split_preserves_pending_prefix() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\x1b["), eq(Vec::<DecodedInput>::new()));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::Needed));
        assert_that!(
            decoder.decode(b"D"),
            eq(vec![DecodedInput::Key(key(
                ClientKeyCode::Left,
                ClientKeyModifiers::NONE,
                b"\x1b[D",
            ))])
        );
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[rstest]
    #[case::plain_enter(b"\x1b[13u", ClientKeyCode::Enter, ClientKeyModifiers::NONE)]
    #[case::shift_enter(b"\x1b[13;2u", ClientKeyCode::Enter, ClientKeyModifiers::SHIFT)]
    #[case::shift_tab(b"\x1b[9;2u", ClientKeyCode::Tab, ClientKeyModifiers::SHIFT)]
    #[case::alt_backspace(b"\x1b[127;3u", ClientKeyCode::Backspace, ClientKeyModifiers::ALT)]
    #[case::shift_backspace(b"\x1b[127;2u", ClientKeyCode::Backspace, ClientKeyModifiers::SHIFT)]
    #[case::ctrl_l(b"\x1b[108;5u", ClientKeyCode::Char('l'), self::modifiers(false, false, true))]
    #[case::ctrl_k(b"\x1b[107;5u", ClientKeyCode::Char('k'), self::modifiers(false, false, true))]
    #[case::shift_alt_one(b"\x1b[49;4u", ClientKeyCode::Char('1'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::unknown_modified_key(b"\x1b[999;2u", ClientKeyCode::Unknown, ClientKeyModifiers::SHIFT)]
    #[case::unsupported_modifier_bits(b"\x1b[118;12u", ClientKeyCode::Unknown, ClientKeyModifiers::NONE)]
    fn test_input_decoder_decode_when_kitty_key_arrives_returns_key(
        #[case] bytes: &[u8],
        #[case] code: ClientKeyCode,
        #[case] modifiers: ClientKeyModifiers,
    ) {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(bytes),
            eq(vec![DecodedInput::Key(key(code, modifiers, bytes))])
        );
    }

    #[rstest]
    #[case::one(b"\x1b!", '1')]
    #[case::two(b"\x1b@", '2')]
    #[case::three(b"\x1b#", '3')]
    #[case::four(b"\x1b$", '4')]
    #[case::five(b"\x1b%", '5')]
    #[case::six(b"\x1b^", '6')]
    #[case::seven(b"\x1b&", '7')]
    #[case::eight(b"\x1b*", '8')]
    #[case::nine(b"\x1b(", '9')]
    fn test_input_decoder_decode_when_legacy_shift_alt_digit_arrives_returns_key(
        #[case] bytes: &[u8],
        #[case] character: char,
    ) {
        let mut decoder = InputDecoder::default();

        assert_that!(
            self::decode_and_finalize(&mut decoder, bytes),
            eq(vec![DecodedInput::Key(key(
                ClientKeyCode::Char(character),
                ClientKeyModifiers::SHIFT_ALT,
                bytes,
            ))])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_kitty_key_is_split_preserves_pending_prefix() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\x1b[13"), eq(Vec::<DecodedInput>::new()));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::Needed));
        assert_that!(
            decoder.decode(b";2u"),
            eq(vec![DecodedInput::Key(key(
                ClientKeyCode::Enter,
                ClientKeyModifiers::SHIFT,
                b"\x1b[13;2u",
            ))])
        );
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[test]
    fn test_input_decoder_decode_when_bracketed_paste_arrives_returns_single_paste() {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(b"\x1b[200~echo hi\n\x1b[201~"),
            eq(vec![DecodedInput::Paste(b"echo hi\n".to_vec())])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_bracketed_paste_is_split_preserves_pending_paste() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\x1b[200~echo"), eq(Vec::<DecodedInput>::new()));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
        assert_that!(
            decoder.decode(b" hi\n\x1b[201~"),
            eq(vec![DecodedInput::Paste(b"echo hi\n".to_vec())])
        );
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[rstest]
    #[case::bare_escape(b"\x1b")]
    #[case::incomplete_csi(b"\x1b[")]
    fn test_input_decoder_needs_idle_timeout_when_escape_prefix_is_pending(#[case] bytes: &[u8]) {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(bytes), eq(Vec::<DecodedInput>::new()));

        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::Needed));
    }

    #[test]
    fn test_input_decoder_when_osc_payload_is_split_after_idle_preserves_payload() {
        let mut decoder = InputDecoder::default();

        assert_that!(decoder.decode(b"\x1b]0;"), eq(Vec::<DecodedInput>::new()));
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
        assert_that!(
            decoder.decode(b"\x1bC\x07"),
            eq(vec![DecodedInput::Input(b"\x1b]0;\x1bC\x07".to_vec())])
        );
        assert_that!(decoder.idle_timeout(), eq(InputIdleTimeout::NotNeeded));
    }

    #[rstest]
    #[case::wheel_up(b"\x1b[<64;10;5M", 64)]
    #[case::wheel_down(b"\x1b[<65;10;5M", 65)]
    fn test_input_decoder_decode_when_mouse_wheel_arrives_returns_mouse_event(
        #[case] bytes: &[u8],
        #[case] button: u16,
    ) {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(bytes),
            eq(vec![DecodedInput::Mouse(ClientMouseEvent {
                button,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition { row: 4, col: 9 },
            })])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_mouse_click_arrives_returns_mouse_event() {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(b"\x1b[<0;10;5M"),
            eq(vec![DecodedInput::Mouse(ClientMouseEvent {
                button: 0,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition { row: 4, col: 9 },
            })])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_sgr_alt_mouse_click_arrives_returns_alt_mouse_event() {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(b"\x1b[<8;10;5M"),
            eq(vec![DecodedInput::Mouse(ClientMouseEvent {
                button: 8,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition { row: 4, col: 9 },
            })])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_sgr_alt_mouse_release_arrives_returns_alt_mouse_event() {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(b"\x1b[<8;10;5m"),
            eq(vec![DecodedInput::Mouse(ClientMouseEvent {
                button: 8,
                phase: ClientMouseEventPhase::Release,
                position: ClientMousePosition { row: 4, col: 9 },
            })])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_mouse_drag_arrives_returns_mouse_event() {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(b"\x1b[<32;10;5M"),
            eq(vec![DecodedInput::Mouse(ClientMouseEvent {
                button: 32,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition { row: 4, col: 9 },
            })])
        );
    }

    #[test]
    fn test_input_decoder_decode_when_mouse_release_arrives_returns_mouse_event() {
        let mut decoder = InputDecoder::default();

        assert_that!(
            decoder.decode(b"\x1b[<0;10;5m"),
            eq(vec![DecodedInput::Mouse(ClientMouseEvent {
                button: 0,
                phase: ClientMouseEventPhase::Release,
                position: ClientMousePosition { row: 4, col: 9 },
            })])
        );
    }

    fn decode_and_finalize(decoder: &mut InputDecoder, bytes: &[u8]) -> Vec<DecodedInput> {
        let mut events = decoder.decode(bytes);
        events.extend(decoder.finalize());
        events
    }

    const fn modifiers(shift: bool, alt: bool, ctrl: bool) -> ClientKeyModifiers {
        ClientKeyModifiers { alt, ctrl, shift }
    }
}
