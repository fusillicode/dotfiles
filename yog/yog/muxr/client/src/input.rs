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
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedInput {
    CopySelection,
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
    Paste(Vec<u8>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SgrMouseEvent {
    Event(ClientMouseEvent),
    Ignored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KittyKeyModifiers {
    Supported(ClientKeyModifiers),
    Unsupported,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputDecoder {
    pending: PendingInput,
}

impl InputDecoder {
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

        match std::mem::take(&mut self.pending) {
            PendingInput::None => {}
            PendingInput::EscapeSequence(bytes) if bytes.as_slice() == [ESC] => {
                self::push_key(
                    &mut decoded,
                    &mut input,
                    self::key(ClientKeyCode::Esc, ClientKeyModifiers::NONE, &bytes),
                );
            }
            PendingInput::EscapeSequence(bytes) => input.extend(bytes),
            PendingInput::Paste(bytes) => {
                input.extend(BRACKETED_PASTE_START);
                input.extend(bytes);
            }
        }

        self::push_input(&mut decoded, &mut input);
        decoded
    }

    #[must_use]
    pub const fn needs_idle_timeout(&self) -> bool {
        matches!(self.pending, PendingInput::EscapeSequence(_))
    }

    fn push_byte(&mut self, byte: u8, input: &mut Vec<u8>, decoded: &mut Vec<DecodedInput>) {
        if let PendingInput::Paste(bytes) = &mut self.pending {
            bytes.push(byte);
            if bytes.ends_with(BRACKETED_PASTE_END) {
                let paste_len = bytes.len().saturating_sub(BRACKETED_PASTE_END.len());
                bytes.truncate(paste_len);
                let PendingInput::Paste(bytes) = std::mem::take(&mut self.pending) else {
                    return;
                };
                decoded.push(DecodedInput::Paste(bytes));
            }
            return;
        }

        if let PendingInput::EscapeSequence(bytes) = &mut self.pending {
            bytes.push(byte);
            if !self::is_pending_escape_complete(bytes) {
                return;
            }

            let PendingInput::EscapeSequence(bytes) = std::mem::take(&mut self.pending) else {
                return;
            };
            if bytes == BRACKETED_PASTE_START {
                self::push_input(decoded, input);
                self.pending = PendingInput::Paste(Vec::new());
            } else {
                self::finish_escape_sequence(bytes, input, decoded);
            }
            return;
        }

        if byte == ESC {
            self.pending = PendingInput::EscapeSequence(vec![ESC]);
            return;
        }

        if let Some(key) = self::key_for_plain_byte(byte) {
            self::push_key(decoded, input, key);
            return;
        }

        input.push(byte);
    }
}

fn finish_escape_sequence(bytes: Vec<u8>, input: &mut Vec<u8>, decoded: &mut Vec<DecodedInput>) {
    if let [ESC, byte] = bytes.as_slice()
        && *byte == b'C'
    {
        self::push_input(decoded, input);
        decoded.push(DecodedInput::CopySelection);
        return;
    }

    if let [ESC, byte] = bytes.as_slice()
        && let Some(key) = self::key_for_escaped_byte(*byte)
    {
        self::push_key(decoded, input, key);
        return;
    }

    if let Some(key) = self::key_for_csi_sequence(&bytes) {
        if self::is_copy_selection_key(&key) {
            self::push_input(decoded, input);
            decoded.push(DecodedInput::CopySelection);
        } else {
            self::push_key(decoded, input, key);
        }
        return;
    }

    if let Some(event) = self::sgr_mouse_event(&bytes) {
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
    let code = match byte {
        b'h' | b'j' | b'k' | b'l' => ClientKeyCode::Char(char::from(byte)),
        _ => return None,
    };

    Some(self::key(code, ClientKeyModifiers::NONE, &[byte]))
}

fn key_for_escaped_byte(byte: u8) -> Option<ClientKey> {
    let (code, modifiers) = match byte {
        CTRL_N => (ClientKeyCode::Char('n'), ClientKeyModifiers::CTRL_ALT),
        CTRL_P => (ClientKeyCode::Char('p'), ClientKeyModifiers::CTRL_ALT),
        b'D' | b'E' | b'F' | b'H' | b'J' | b'K' | b'L' | b'N' | b'P' | b'R' | b'S' | b'V' | b'W' => {
            (ClientKeyCode::Char(char::from(byte)), ClientKeyModifiers::SHIFT_ALT)
        }
        _ => return None,
    };

    Some(self::key(code, modifiers, &[ESC, byte]))
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

fn is_copy_selection_key(key: &ClientKey) -> bool {
    matches!(key.code, ClientKeyCode::Char('C')) && key.modifiers == ClientKeyModifiers::SHIFT_ALT
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
        Some(raw) => self::kitty_key_modifiers(raw)?,
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

fn kitty_key_modifiers(raw: &[u8]) -> Option<KittyKeyModifiers> {
    let flags = self::parse_mouse_number(raw)?.checked_sub(1)?;
    if flags & !0b111 != 0 {
        return Some(KittyKeyModifiers::Unsupported);
    }
    Some(KittyKeyModifiers::Supported(ClientKeyModifiers {
        alt: flags & 0b010 != 0,
        ctrl: flags & 0b100 != 0,
        shift: flags & 0b001 != 0,
    }))
}

fn sgr_mouse_event(bytes: &[u8]) -> Option<SgrMouseEvent> {
    if bytes.first() != Some(&ESC) || bytes.get(1) != Some(&b'[') || bytes.get(2) != Some(&b'<') {
        return None;
    }
    let release = match bytes.last() {
        Some(b'M') => false,
        Some(b'm') => true,
        Some(_) | None => return Some(SgrMouseEvent::Ignored),
    };
    let phase = if release {
        ClientMouseEventPhase::Release
    } else {
        ClientMouseEventPhase::Press
    };
    let Some((button, position)) = self::sgr_mouse_button_and_position(bytes) else {
        return Some(SgrMouseEvent::Ignored);
    };
    Some(SgrMouseEvent::Event(ClientMouseEvent {
        button,
        phase,
        position,
    }))
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

fn is_pending_escape_complete(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_PENDING_ESCAPE_BYTES {
        return true;
    }

    match bytes {
        [ESC] | [ESC, b'['] | [ESC, b'[', b'<'] => false,
        [ESC, b'[', rest @ ..] => rest.last().is_some_and(|byte| (0x40..=0x7e).contains(byte)),
        _ => true,
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

    use super::*;

    #[test]
    fn test_input_decoder_decode_when_bytes_are_plain_returns_input() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"abc"), vec![DecodedInput::Input(b"abc".to_vec())]);
    }

    #[test]
    fn test_input_decoder_decode_when_bare_enter_arrives_preserves_input_bytes() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\r"), vec![DecodedInput::Input(b"\r".to_vec())]);
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

        pretty_assertions::assert_eq!(
            decoder.decode(bytes),
            vec![DecodedInput::Key(key(code, modifiers, bytes))]
        );
    }

    #[rstest]
    #[case::legacy(b"\x1bC")]
    #[case::kitty(b"\x1b[99;4u")]
    fn test_input_decoder_decode_when_copy_shortcut_arrives_returns_copy_selection(#[case] bytes: &[u8]) {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(bytes), vec![DecodedInput::CopySelection]);
    }

    #[test]
    fn test_input_decoder_decode_when_shortcut_is_between_input_splits_actions() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(b"a\x1bEb"),
            vec![
                DecodedInput::Input(b"a".to_vec()),
                DecodedInput::Key(key(ClientKeyCode::Char('E'), ClientKeyModifiers::SHIFT_ALT, b"\x1bE",)),
                DecodedInput::Input(b"b".to_vec()),
            ],
        );
    }

    #[rstest]
    #[case::unknown_escape(b"\x1bX")]
    #[case::unknown_csi(b"\x1b[1~")]
    fn test_input_decoder_decode_when_escape_is_not_muxr_cmd_preserves_bytes(#[case] bytes: &[u8]) {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(bytes), vec![DecodedInput::Input(bytes.to_vec())]);
    }

    #[test]
    fn test_input_decoder_decode_when_shortcut_is_split_preserves_pending_prefix() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\x1b"), Vec::<DecodedInput>::new());
        assert2::assert!(decoder.needs_idle_timeout());
        pretty_assertions::assert_eq!(
            decoder.decode(b"E"),
            vec![DecodedInput::Key(key(
                ClientKeyCode::Char('E'),
                ClientKeyModifiers::SHIFT_ALT,
                b"\x1bE",
            ))]
        );
        assert2::assert!(!decoder.needs_idle_timeout());
    }

    #[test]
    fn test_input_decoder_finalize_when_bare_escape_arrives_returns_key() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\x1b"), Vec::<DecodedInput>::new());
        assert2::assert!(decoder.needs_idle_timeout());
        pretty_assertions::assert_eq!(
            decoder.finalize(),
            vec![DecodedInput::Key(key(
                ClientKeyCode::Esc,
                ClientKeyModifiers::NONE,
                b"\x1b",
            ))]
        );
        assert2::assert!(!decoder.needs_idle_timeout());
    }

    #[test]
    fn test_input_decoder_finalize_when_pending_unknown_sequence_arrives_preserves_bytes() {
        let mut decoder = InputDecoder::default();
        let bytes = b"\x1b[1";

        pretty_assertions::assert_eq!(decoder.decode(bytes), Vec::<DecodedInput>::new());
        assert2::assert!(decoder.needs_idle_timeout());
        pretty_assertions::assert_eq!(decoder.finalize(), vec![DecodedInput::Input(bytes.to_vec())]);
        assert2::assert!(!decoder.needs_idle_timeout());
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

        pretty_assertions::assert_eq!(
            decoder.decode(bytes),
            vec![DecodedInput::Key(key(code, ClientKeyModifiers::NONE, bytes))]
        );
    }

    #[test]
    fn test_input_decoder_decode_when_arrow_is_split_preserves_pending_prefix() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\x1b["), Vec::<DecodedInput>::new());
        assert2::assert!(decoder.needs_idle_timeout());
        pretty_assertions::assert_eq!(
            decoder.decode(b"D"),
            vec![DecodedInput::Key(key(
                ClientKeyCode::Left,
                ClientKeyModifiers::NONE,
                b"\x1b[D",
            ))]
        );
        assert2::assert!(!decoder.needs_idle_timeout());
    }

    #[rstest]
    #[case::plain_enter(b"\x1b[13u", ClientKeyCode::Enter, ClientKeyModifiers::NONE)]
    #[case::shift_enter(b"\x1b[13;2u", ClientKeyCode::Enter, ClientKeyModifiers::SHIFT)]
    #[case::shift_tab(b"\x1b[9;2u", ClientKeyCode::Tab, ClientKeyModifiers::SHIFT)]
    #[case::shift_backspace(b"\x1b[127;2u", ClientKeyCode::Backspace, ClientKeyModifiers::SHIFT)]
    #[case::ctrl_l(b"\x1b[108;5u", ClientKeyCode::Char('l'), self::modifiers(false, false, true))]
    #[case::ctrl_k(b"\x1b[107;5u", ClientKeyCode::Char('k'), self::modifiers(false, false, true))]
    #[case::unknown_modified_key(b"\x1b[999;2u", ClientKeyCode::Unknown, ClientKeyModifiers::SHIFT)]
    #[case::unsupported_modifier_bits(b"\x1b[118;12u", ClientKeyCode::Unknown, ClientKeyModifiers::NONE)]
    fn test_input_decoder_decode_when_kitty_key_arrives_returns_key(
        #[case] bytes: &[u8],
        #[case] code: ClientKeyCode,
        #[case] modifiers: ClientKeyModifiers,
    ) {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(bytes),
            vec![DecodedInput::Key(key(code, modifiers, bytes))]
        );
    }

    #[test]
    fn test_input_decoder_decode_when_kitty_key_is_split_preserves_pending_prefix() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\x1b[13"), Vec::<DecodedInput>::new());
        assert2::assert!(decoder.needs_idle_timeout());
        pretty_assertions::assert_eq!(
            decoder.decode(b";2u"),
            vec![DecodedInput::Key(key(
                ClientKeyCode::Enter,
                ClientKeyModifiers::SHIFT,
                b"\x1b[13;2u",
            ))]
        );
        assert2::assert!(!decoder.needs_idle_timeout());
    }

    #[test]
    fn test_input_decoder_decode_when_bracketed_paste_arrives_returns_single_paste() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(b"\x1b[200~echo hi\n\x1b[201~"),
            vec![DecodedInput::Paste(b"echo hi\n".to_vec())]
        );
    }

    #[test]
    fn test_input_decoder_decode_when_bracketed_paste_is_split_preserves_pending_paste() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\x1b[200~echo"), Vec::<DecodedInput>::new());
        assert2::assert!(!decoder.needs_idle_timeout());
        pretty_assertions::assert_eq!(
            decoder.decode(b" hi\n\x1b[201~"),
            vec![DecodedInput::Paste(b"echo hi\n".to_vec())]
        );
        assert2::assert!(!decoder.needs_idle_timeout());
    }

    #[rstest]
    #[case::bare_escape(b"\x1b")]
    #[case::incomplete_csi(b"\x1b[")]
    fn test_input_decoder_needs_idle_timeout_when_escape_prefix_is_pending(#[case] bytes: &[u8]) {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(bytes), Vec::<DecodedInput>::new());

        assert2::assert!(decoder.needs_idle_timeout());
    }

    #[rstest]
    #[case::wheel_up(b"\x1b[<64;10;5M", 64)]
    #[case::wheel_down(b"\x1b[<65;10;5M", 65)]
    fn test_input_decoder_decode_when_mouse_wheel_arrives_returns_mouse_event(
        #[case] bytes: &[u8],
        #[case] button: u16,
    ) {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(bytes),
            vec![DecodedInput::Mouse(ClientMouseEvent {
                button,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition { row: 4, col: 9 },
            })]
        );
    }

    #[test]
    fn test_input_decoder_decode_when_mouse_click_arrives_returns_mouse_event() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(b"\x1b[<0;10;5M"),
            vec![DecodedInput::Mouse(ClientMouseEvent {
                button: 0,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition { row: 4, col: 9 },
            })]
        );
    }

    #[test]
    fn test_input_decoder_decode_when_mouse_drag_arrives_returns_mouse_event() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(b"\x1b[<32;10;5M"),
            vec![DecodedInput::Mouse(ClientMouseEvent {
                button: 32,
                phase: ClientMouseEventPhase::Press,
                position: ClientMousePosition { row: 4, col: 9 },
            })]
        );
    }

    #[test]
    fn test_input_decoder_decode_when_mouse_release_arrives_returns_mouse_event() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(b"\x1b[<0;10;5m"),
            vec![DecodedInput::Mouse(ClientMouseEvent {
                button: 0,
                phase: ClientMouseEventPhase::Release,
                position: ClientMousePosition { row: 4, col: 9 },
            })]
        );
    }

    const fn modifiers(shift: bool, alt: bool, ctrl: bool) -> ClientKeyModifiers {
        ClientKeyModifiers { alt, ctrl, shift }
    }
}
