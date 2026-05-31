use muxr_core::ClientKey;
use muxr_core::ClientKeyCode;
use muxr_core::ClientKeyModifiers;
use muxr_core::ClientMousePosition;
use muxr_core::PaneScrollDirection;

const CTRL_N: u8 = 0x0e;
const CTRL_P: u8 = 0x10;
const ESC: u8 = 0x1b;
const MAX_PENDING_ESCAPE_BYTES: usize = 64;
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedInput {
    Input(Vec<u8>),
    Key(ClientKey),
    MouseFocus(ClientMousePosition),
    Paste(Vec<u8>),
    Scroll {
        position: ClientMousePosition,
        direction: PaneScrollDirection,
    },
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
    Focus(ClientMousePosition),
    Ignored,
    Scroll {
        position: ClientMousePosition,
        direction: PaneScrollDirection,
    },
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
    pub fn has_pending(&self) -> bool {
        self.pending != PendingInput::None
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
        && let Some(key) = self::key_for_escaped_byte(*byte)
    {
        self::push_key(decoded, input, key);
        return;
    }

    if let Some(key) = self::key_for_csi_sequence(&bytes) {
        self::push_key(decoded, input, key);
        return;
    }

    if let Some(event) = self::sgr_mouse_event(&bytes) {
        self::push_input(decoded, input);
        match event {
            SgrMouseEvent::Focus(position) => decoded.push(DecodedInput::MouseFocus(position)),
            SgrMouseEvent::Ignored => {}
            SgrMouseEvent::Scroll { position, direction } => decoded.push(DecodedInput::Scroll { position, direction }),
        }
        return;
    }

    input.extend(bytes);
}

fn key_for_plain_byte(byte: u8) -> Option<ClientKey> {
    match byte {
        b'h' => Some(self::key(ClientKeyCode::Char('h'), ClientKeyModifiers::NONE, &[byte])),
        b'j' => Some(self::key(ClientKeyCode::Char('j'), ClientKeyModifiers::NONE, &[byte])),
        b'k' => Some(self::key(ClientKeyCode::Char('k'), ClientKeyModifiers::NONE, &[byte])),
        b'l' => Some(self::key(ClientKeyCode::Char('l'), ClientKeyModifiers::NONE, &[byte])),
        _ => None,
    }
}

fn key_for_escaped_byte(byte: u8) -> Option<ClientKey> {
    match byte {
        CTRL_N => Some(self::key(
            ClientKeyCode::Char('n'),
            ClientKeyModifiers::CTRL_ALT,
            &[ESC, byte],
        )),
        CTRL_P => Some(self::key(
            ClientKeyCode::Char('p'),
            ClientKeyModifiers::CTRL_ALT,
            &[ESC, byte],
        )),
        b'D' => Some(self::key(
            ClientKeyCode::Char('D'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'E' => Some(self::key(
            ClientKeyCode::Char('E'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'H' => Some(self::key(
            ClientKeyCode::Char('H'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'J' => Some(self::key(
            ClientKeyCode::Char('J'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'K' => Some(self::key(
            ClientKeyCode::Char('K'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'L' => Some(self::key(
            ClientKeyCode::Char('L'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'N' => Some(self::key(
            ClientKeyCode::Char('N'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'P' => Some(self::key(
            ClientKeyCode::Char('P'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'R' => Some(self::key(
            ClientKeyCode::Char('R'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'V' => Some(self::key(
            ClientKeyCode::Char('V'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        b'W' => Some(self::key(
            ClientKeyCode::Char('W'),
            ClientKeyModifiers::SHIFT_ALT,
            &[ESC, byte],
        )),
        _ => None,
    }
}

fn key_for_csi_sequence(bytes: &[u8]) -> Option<ClientKey> {
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

fn sgr_mouse_event(bytes: &[u8]) -> Option<SgrMouseEvent> {
    if bytes.first() != Some(&ESC) || bytes.get(1) != Some(&b'[') || bytes.get(2) != Some(&b'<') {
        return None;
    }
    if bytes.last() != Some(&b'M') {
        return Some(SgrMouseEvent::Ignored);
    }
    let Some(body_end) = bytes.len().checked_sub(1) else {
        return Some(SgrMouseEvent::Ignored);
    };
    let Some(body) = bytes.get(3..body_end) else {
        return Some(SgrMouseEvent::Ignored);
    };
    let mut parts = body.split(|byte| *byte == b';');
    let Some(button) = parts.next().and_then(self::parse_mouse_number) else {
        return Some(SgrMouseEvent::Ignored);
    };
    let Some(col) = parts
        .next()
        .and_then(self::parse_mouse_number)
        .and_then(|col| col.checked_sub(1))
    else {
        return Some(SgrMouseEvent::Ignored);
    };
    let Some(row) = parts
        .next()
        .and_then(self::parse_mouse_number)
        .and_then(|row| row.checked_sub(1))
    else {
        return Some(SgrMouseEvent::Ignored);
    };
    if parts.next().is_some() {
        return Some(SgrMouseEvent::Ignored);
    }

    let position = ClientMousePosition::new(row, col);
    match button {
        0 => Some(SgrMouseEvent::Focus(position)),
        64 => Some(SgrMouseEvent::Scroll {
            position,
            direction: PaneScrollDirection::Up,
        }),
        65 => Some(SgrMouseEvent::Scroll {
            position,
            direction: PaneScrollDirection::Down,
        }),
        _ => Some(SgrMouseEvent::Ignored),
    }
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
        [ESC, b'[', rest @ ..] => rest.last().is_some_and(|byte| is_csi_final_byte(*byte)),
        _ => true,
    }
}

fn is_csi_final_byte(byte: u8) -> bool {
    (0x40..=0x7e).contains(&byte)
}

fn push_input(decoded: &mut Vec<DecodedInput>, input: &mut Vec<u8>) {
    if input.is_empty() {
        return;
    }

    decoded.push(DecodedInput::Input(std::mem::take(input)));
}

fn key(code: ClientKeyCode, modifiers: ClientKeyModifiers, raw_bytes: &[u8]) -> ClientKey {
    ClientKey::new(code, modifiers, raw_bytes.to_vec())
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
    #[case::close_pane(b"\x1bW", ClientKeyCode::Char('W'), ClientKeyModifiers::SHIFT_ALT)]
    #[case::enter_resize_mode(b"\x1bR", ClientKeyCode::Char('R'), ClientKeyModifiers::SHIFT_ALT)]
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
    fn test_input_decoder_decode_when_escape_is_not_muxr_command_preserves_bytes(#[case] bytes: &[u8]) {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(bytes), vec![DecodedInput::Input(bytes.to_vec())]);
    }

    #[test]
    fn test_input_decoder_decode_when_shortcut_is_split_preserves_pending_prefix() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\x1b"), Vec::<DecodedInput>::new());
        assert2::assert!(decoder.has_pending());
        pretty_assertions::assert_eq!(
            decoder.decode(b"E"),
            vec![DecodedInput::Key(key(
                ClientKeyCode::Char('E'),
                ClientKeyModifiers::SHIFT_ALT,
                b"\x1bE",
            ))]
        );
        assert2::assert!(!decoder.has_pending());
    }

    #[test]
    fn test_input_decoder_finalize_when_bare_escape_arrives_returns_key() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\x1b"), Vec::<DecodedInput>::new());
        assert2::assert!(decoder.has_pending());
        pretty_assertions::assert_eq!(
            decoder.finalize(),
            vec![DecodedInput::Key(key(
                ClientKeyCode::Esc,
                ClientKeyModifiers::NONE,
                b"\x1b",
            ))]
        );
        assert2::assert!(!decoder.has_pending());
    }

    #[test]
    fn test_input_decoder_finalize_when_pending_unknown_sequence_arrives_preserves_bytes() {
        let mut decoder = InputDecoder::default();
        let bytes = b"\x1b[1";

        pretty_assertions::assert_eq!(decoder.decode(bytes), Vec::<DecodedInput>::new());
        assert2::assert!(decoder.has_pending());
        pretty_assertions::assert_eq!(decoder.finalize(), vec![DecodedInput::Input(bytes.to_vec())]);
        assert2::assert!(!decoder.has_pending());
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
        assert2::assert!(decoder.has_pending());
        pretty_assertions::assert_eq!(
            decoder.decode(b"D"),
            vec![DecodedInput::Key(key(
                ClientKeyCode::Left,
                ClientKeyModifiers::NONE,
                b"\x1b[D",
            ))]
        );
        assert2::assert!(!decoder.has_pending());
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
        assert2::assert!(decoder.has_pending());
        pretty_assertions::assert_eq!(
            decoder.decode(b" hi\n\x1b[201~"),
            vec![DecodedInput::Paste(b"echo hi\n".to_vec())]
        );
        assert2::assert!(!decoder.has_pending());
    }

    #[rstest]
    #[case::wheel_up(b"\x1b[<64;10;5M", PaneScrollDirection::Up)]
    #[case::wheel_down(b"\x1b[<65;10;5M", PaneScrollDirection::Down)]
    fn test_input_decoder_decode_when_mouse_wheel_arrives_returns_scroll(
        #[case] bytes: &[u8],
        #[case] direction: PaneScrollDirection,
    ) {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(bytes),
            vec![DecodedInput::Scroll {
                position: ClientMousePosition::new(4, 9),
                direction,
            }]
        );
    }

    #[test]
    fn test_input_decoder_decode_when_mouse_click_arrives_returns_focus_position() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(
            decoder.decode(b"\x1b[<0;10;5M"),
            vec![DecodedInput::MouseFocus(ClientMousePosition::new(4, 9))]
        );
    }

    #[test]
    fn test_input_decoder_decode_when_mouse_release_arrives_consumes_event() {
        let mut decoder = InputDecoder::default();

        pretty_assertions::assert_eq!(decoder.decode(b"\x1b[<0;10;5m"), Vec::<DecodedInput>::new());
    }
}
