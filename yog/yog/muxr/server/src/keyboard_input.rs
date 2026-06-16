use muxr_core::ClientKey;
use muxr_core::ClientKeyCode;
use muxr_core::ClientKeyModifiers;

use crate::pane::borders::BorderRenderMode;
use crate::pane::focus::PaneFocusDirection;
use crate::pane::resize::PaneResizeDirection;
use crate::pane::split::PaneSplitAxis;
use crate::pane::tracked_process::TrackedProcessUserInteraction;
use crate::terminal::TerminalKeyboardProtocol;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientCmd {
    ClosePane,
    EnterResizeMode,
    ExitMode,
    FocusPane(PaneFocusDirection),
    OpenScrollbackEditor,
    ResizePane(PaneResizeDirection),
    SplitPane(PaneSplitAxis),
    Tab(TabCmd),
    TogglePaneFullscreen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabCmd {
    Create,
    FocusNext,
    FocusPrevious,
    MoveNext,
    MovePrevious,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ServerInputMode {
    #[default]
    Normal,
    Resize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyResolution {
    Cmd(ClientCmd),
    Raw,
}

pub const fn border_render_mode(input_mode: ServerInputMode) -> BorderRenderMode {
    match input_mode {
        ServerInputMode::Normal => BorderRenderMode::Focus,
        ServerInputMode::Resize => BorderRenderMode::Resize,
    }
}

pub const fn resolve_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match input_mode {
        ServerInputMode::Normal => self::resolve_normal_key(input_mode, key),
        ServerInputMode::Resize => self::resolve_resize_key(input_mode, key),
    }
}

pub fn input_interaction(bytes: &[u8]) -> TrackedProcessUserInteraction {
    if bytes.contains(&b'\r') || bytes.contains(&b'\n') {
        TrackedProcessUserInteraction::StartsTrackedProcessWork
    } else {
        TrackedProcessUserInteraction::MayEcho
    }
}

pub fn key_input_interaction(key: &ClientKey, bytes: &[u8]) -> TrackedProcessUserInteraction {
    if matches!(key.code, ClientKeyCode::Enter) && key.modifiers == ClientKeyModifiers::NONE {
        TrackedProcessUserInteraction::StartsTrackedProcessWork
    } else {
        self::input_interaction(bytes)
    }
}

pub fn pane_key_input_bytes(key: &ClientKey, keyboard_protocol: TerminalKeyboardProtocol) -> Option<Vec<u8>> {
    match keyboard_protocol {
        TerminalKeyboardProtocol::Legacy => self::legacy_key_input_bytes(key),
        TerminalKeyboardProtocol::KittyLevelOne => self::kitty_key_input_bytes(key),
    }
}

const fn resolve_normal_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match (&key.code, key.modifiers) {
        (ClientKeyCode::Char('E'), ClientKeyModifiers::SHIFT_ALT) => KeyResolution::Cmd(ClientCmd::Tab(TabCmd::Create)),
        (ClientKeyCode::Char('P'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::Tab(TabCmd::FocusPrevious))
        }
        (ClientKeyCode::Char('N'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::Tab(TabCmd::FocusNext))
        }
        (ClientKeyCode::Char('p'), ClientKeyModifiers::CTRL_ALT) => {
            KeyResolution::Cmd(ClientCmd::Tab(TabCmd::MovePrevious))
        }
        (ClientKeyCode::Char('n'), ClientKeyModifiers::CTRL_ALT) => {
            KeyResolution::Cmd(ClientCmd::Tab(TabCmd::MoveNext))
        }
        (ClientKeyCode::Char('H'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::FocusPane(PaneFocusDirection::Left))
        }
        (ClientKeyCode::Char('J'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::FocusPane(PaneFocusDirection::Down))
        }
        (ClientKeyCode::Char('K'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::FocusPane(PaneFocusDirection::Up))
        }
        (ClientKeyCode::Char('L'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::FocusPane(PaneFocusDirection::Right))
        }
        (ClientKeyCode::Char('V'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::SplitPane(PaneSplitAxis::Vertical))
        }
        (ClientKeyCode::Char('D'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::SplitPane(PaneSplitAxis::Horizontal))
        }
        (ClientKeyCode::Char('F'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::TogglePaneFullscreen)
        }
        (ClientKeyCode::Char('W'), ClientKeyModifiers::SHIFT_ALT) => KeyResolution::Cmd(ClientCmd::ClosePane),
        (ClientKeyCode::Char('R'), ClientKeyModifiers::SHIFT_ALT) => {
            *input_mode = ServerInputMode::Resize;
            KeyResolution::Cmd(ClientCmd::EnterResizeMode)
        }
        (ClientKeyCode::Char('S'), ClientKeyModifiers::SHIFT_ALT) => {
            KeyResolution::Cmd(ClientCmd::OpenScrollbackEditor)
        }
        _ => KeyResolution::Raw,
    }
}

fn legacy_key_input_bytes(key: &ClientKey) -> Option<Vec<u8>> {
    match (key.code, key.modifiers) {
        (ClientKeyCode::Backspace, ClientKeyModifiers::NONE) => Some(b"\x7f".to_vec()),
        (ClientKeyCode::Down, ClientKeyModifiers::NONE) => Some(b"\x1b[B".to_vec()),
        (ClientKeyCode::Enter, ClientKeyModifiers::NONE) => Some(b"\r".to_vec()),
        (ClientKeyCode::Esc, ClientKeyModifiers::NONE) => Some(b"\x1b".to_vec()),
        (ClientKeyCode::Left, ClientKeyModifiers::NONE) => Some(b"\x1b[D".to_vec()),
        (ClientKeyCode::Right, ClientKeyModifiers::NONE) => Some(b"\x1b[C".to_vec()),
        (ClientKeyCode::Tab, ClientKeyModifiers::NONE) => Some(b"\t".to_vec()),
        (ClientKeyCode::Tab, ClientKeyModifiers::SHIFT) => Some(b"\x1b[Z".to_vec()),
        (ClientKeyCode::Up, ClientKeyModifiers::NONE) => Some(b"\x1b[A".to_vec()),
        (ClientKeyCode::Char(character), _) if self::raw_bytes_are_kitty_keyboard_sequence(&key.raw_bytes) => {
            self::legacy_kitty_char_input_bytes(character, key.modifiers)
        }
        (ClientKeyCode::Backspace, ClientKeyModifiers::ALT)
            if self::raw_bytes_are_kitty_keyboard_sequence(&key.raw_bytes) =>
        {
            // Alt-Backspace has a stable legacy form used by shells and tmux for backward word deletion.
            Some(b"\x1b\x7f".to_vec())
        }
        (ClientKeyCode::Backspace, ClientKeyModifiers::SHIFT)
            if self::raw_bytes_are_kitty_keyboard_sequence(&key.raw_bytes) =>
        {
            // Legacy panes have no distinct Shift-Backspace form; degrade to DEL so character deletion still works.
            Some(b"\x7f".to_vec())
        }
        _ if !self::raw_bytes_are_kitty_keyboard_sequence(&key.raw_bytes) => Some(key.raw_bytes.clone()),
        // Modified kitty keys cannot be represented in legacy mode without changing semantics, such as Shift-Enter
        // becoming Enter and submitting a prompt. Drop them instead of leaking unfamiliar CSI-u bytes.
        _ => None,
    }
}

fn legacy_kitty_char_input_bytes(character: char, modifiers: ClientKeyModifiers) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    if modifiers.alt {
        bytes.push(b'\x1b');
    }
    if modifiers.ctrl {
        bytes.push(self::legacy_control_byte(character)?);
    } else {
        let mut buffer = [0; 4];
        bytes.extend(character.encode_utf8(&mut buffer).as_bytes());
    }
    Some(bytes)
}

fn legacy_control_byte(character: char) -> Option<u8> {
    let byte = u8::try_from(u32::from(character)).ok()?;
    match byte {
        b'a'..=b'z' => byte.to_ascii_uppercase().checked_sub(b'@'),
        b'A'..=b'Z' => byte.checked_sub(b'@'),
        b' ' => Some(0x00),
        b'[' => Some(0x1b),
        b'\\' => Some(0x1c),
        b']' => Some(0x1d),
        b'^' => Some(0x1e),
        b'_' => Some(0x1f),
        b'?' => Some(0x7f),
        _ => None,
    }
}

fn kitty_key_input_bytes(key: &ClientKey) -> Option<Vec<u8>> {
    if self::raw_bytes_are_kitty_keyboard_sequence(&key.raw_bytes) {
        return Some(key.raw_bytes.clone());
    }

    self::kitty_key_number(key.code)
        .map(|key_number| self::serialize_kitty_key(key_number, key.modifiers))
        .or_else(|| (!key.raw_bytes.is_empty()).then(|| key.raw_bytes.clone()))
}

const fn kitty_key_number(code: ClientKeyCode) -> Option<u16> {
    match code {
        ClientKeyCode::Backspace => Some(127),
        ClientKeyCode::Enter => Some(13),
        ClientKeyCode::Esc => Some(27),
        ClientKeyCode::Tab => Some(9),
        ClientKeyCode::Char(_)
        | ClientKeyCode::Down
        | ClientKeyCode::Left
        | ClientKeyCode::Right
        | ClientKeyCode::Unknown
        | ClientKeyCode::Up => None,
    }
}

fn serialize_kitty_key(key_number: u16, modifiers: ClientKeyModifiers) -> Vec<u8> {
    let modifier_number = self::kitty_modifier_number(modifiers);
    if modifier_number == 1 {
        format!("\x1b[{key_number}u").into_bytes()
    } else {
        format!("\x1b[{key_number};{modifier_number}u").into_bytes()
    }
}

const fn kitty_modifier_number(modifiers: ClientKeyModifiers) -> u8 {
    match (modifiers.shift, modifiers.alt, modifiers.ctrl) {
        (false, false, false) => 1,
        (true, false, false) => 2,
        (false, true, false) => 3,
        (true, true, false) => 4,
        (false, false, true) => 5,
        (true, false, true) => 6,
        (false, true, true) => 7,
        (true, true, true) => 8,
    }
}

fn raw_bytes_are_kitty_keyboard_sequence(bytes: &[u8]) -> bool {
    if bytes.first() != Some(&b'\x1b') || bytes.get(1) != Some(&b'[') || bytes.last() != Some(&b'u') {
        return false;
    }

    let Some(body_end) = bytes.len().checked_sub(1) else {
        return false;
    };
    let Some(body) = bytes.get(2..body_end) else {
        return false;
    };
    let mut parts = body.split(|byte| *byte == b';');
    if parts.next().and_then(self::parse_ascii_u16).is_none() {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, None) => true,
        (Some(modifier), None) => self::parse_ascii_u16(modifier).is_some(),
        (None | Some(_), Some(_)) => false,
    }
}

fn parse_ascii_u16(raw: &[u8]) -> Option<u16> {
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

const fn resolve_resize_key(input_mode: &mut ServerInputMode, key: &ClientKey) -> KeyResolution {
    match (&key.code, key.modifiers) {
        (ClientKeyCode::Esc, ClientKeyModifiers::NONE) => {
            *input_mode = ServerInputMode::Normal;
            KeyResolution::Cmd(ClientCmd::ExitMode)
        }
        (ClientKeyCode::Char('h') | ClientKeyCode::Left, ClientKeyModifiers::NONE) => {
            KeyResolution::Cmd(ClientCmd::ResizePane(PaneResizeDirection::Left))
        }
        (ClientKeyCode::Char('j') | ClientKeyCode::Down, ClientKeyModifiers::NONE) => {
            KeyResolution::Cmd(ClientCmd::ResizePane(PaneResizeDirection::Down))
        }
        (ClientKeyCode::Char('k') | ClientKeyCode::Up, ClientKeyModifiers::NONE) => {
            KeyResolution::Cmd(ClientCmd::ResizePane(PaneResizeDirection::Up))
        }
        (ClientKeyCode::Char('l') | ClientKeyCode::Right, ClientKeyModifiers::NONE) => {
            KeyResolution::Cmd(ClientCmd::ResizePane(PaneResizeDirection::Right))
        }
        _ => KeyResolution::Raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::typing(b"abc", TrackedProcessUserInteraction::MayEcho)]
    #[case::enter(b"\r", TrackedProcessUserInteraction::StartsTrackedProcessWork)]
    #[case::input_with_newline(b"one\ntwo", TrackedProcessUserInteraction::StartsTrackedProcessWork)]
    fn test_input_interaction_when_bytes_vary_classifies_prompt_submission(
        #[case] bytes: &[u8],
        #[case] expected: TrackedProcessUserInteraction,
    ) {
        pretty_assertions::assert_eq!(self::input_interaction(bytes), expected);
    }

    #[rstest::rstest]
    #[case::legacy_plain_enter(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Enter, ClientKeyModifiers::NONE, b"\x1b[13u"),
        Some(b"\r".to_vec())
    )]
    #[case::legacy_shift_enter(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Enter, ClientKeyModifiers::SHIFT, b"\x1b[13;2u"),
        None
    )]
    #[case::legacy_shift_tab(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Tab, ClientKeyModifiers::SHIFT, b"\x1b[9;2u"),
        Some(b"\x1b[Z".to_vec())
    )]
    #[case::legacy_unbound_raw(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Char('x'), ClientKeyModifiers::NONE, b"x"),
        Some(b"x".to_vec())
    )]
    #[case::legacy_plain_char_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Char('x'), ClientKeyModifiers::NONE, b"\x1b[120u"),
        Some(b"x".to_vec())
    )]
    #[case::legacy_shift_char_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Char('X'), ClientKeyModifiers::SHIFT, b"\x1b[120;2u"),
        Some(b"X".to_vec())
    )]
    #[case::legacy_ctrl_l_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Char('l'), self::modifiers(false, false, true), b"\x1b[108;5u"),
        Some(b"\x0c".to_vec())
    )]
    #[case::legacy_ctrl_k_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Char('k'), self::modifiers(false, false, true), b"\x1b[107;5u"),
        Some(b"\x0b".to_vec())
    )]
    #[case::legacy_ctrl_w_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Char('w'), self::modifiers(false, false, true), b"\x1b[119;5u"),
        Some(b"\x17".to_vec())
    )]
    #[case::legacy_alt_f_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Char('f'), ClientKeyModifiers::ALT, b"\x1b[102;3u"),
        Some(b"\x1bf".to_vec())
    )]
    #[case::legacy_alt_backspace_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Backspace, ClientKeyModifiers::ALT, b"\x1b[127;3u"),
        Some(b"\x1b\x7f".to_vec())
    )]
    #[case::legacy_shift_backspace_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Backspace, ClientKeyModifiers::SHIFT, b"\x1b[127;2u"),
        Some(b"\x7f".to_vec())
    )]
    #[case::legacy_ctrl_alt_a_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Char('a'), ClientKeyModifiers::CTRL_ALT, b"\x1b[97;7u"),
        Some(b"\x1b\x01".to_vec())
    )]
    #[case::legacy_unknown_modified_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Unknown, ClientKeyModifiers::SHIFT, b"\x1b[999;2u"),
        None
    )]
    #[case::legacy_unsupported_modifier_bits_kitty(
        TerminalKeyboardProtocol::Legacy,
        self::key(ClientKeyCode::Unknown, ClientKeyModifiers::NONE, b"\x1b[118;12u"),
        None
    )]
    #[case::kitty_shift_enter(
        TerminalKeyboardProtocol::KittyLevelOne,
        self::key(ClientKeyCode::Enter, ClientKeyModifiers::SHIFT, b"\x1b[13;2u"),
        Some(b"\x1b[13;2u".to_vec())
    )]
    #[case::kitty_plain_enter_serialized(
        TerminalKeyboardProtocol::KittyLevelOne,
        self::key(ClientKeyCode::Enter, ClientKeyModifiers::NONE, b"\r"),
        Some(b"\x1b[13u".to_vec())
    )]
    #[case::kitty_unbound_raw(
        TerminalKeyboardProtocol::KittyLevelOne,
        self::key(ClientKeyCode::Char('x'), ClientKeyModifiers::NONE, b"x"),
        Some(b"x".to_vec())
    )]
    #[case::kitty_unknown_modified_kitty(
        TerminalKeyboardProtocol::KittyLevelOne,
        self::key(ClientKeyCode::Unknown, ClientKeyModifiers::SHIFT, b"\x1b[999;2u"),
        Some(b"\x1b[999;2u".to_vec())
    )]
    #[case::kitty_unsupported_modifier_bits_kitty(
        TerminalKeyboardProtocol::KittyLevelOne,
        self::key(ClientKeyCode::Unknown, ClientKeyModifiers::NONE, b"\x1b[118;12u"),
        Some(b"\x1b[118;12u".to_vec())
    )]
    fn test_pane_key_input_bytes_when_keyboard_protocol_varies_returns_expected_bytes(
        #[case] keyboard_protocol: TerminalKeyboardProtocol,
        #[case] key: ClientKey,
        #[case] expected: Option<Vec<u8>>,
    ) {
        pretty_assertions::assert_eq!(self::pane_key_input_bytes(&key, keyboard_protocol), expected);
    }

    #[rstest::rstest]
    #[case::plain_enter(
        self::key(ClientKeyCode::Enter, ClientKeyModifiers::NONE, b"\x1b[13u"),
        b"\x1b[13u",
        TrackedProcessUserInteraction::StartsTrackedProcessWork
    )]
    #[case::shift_enter(
        self::key(ClientKeyCode::Enter, ClientKeyModifiers::SHIFT, b"\x1b[13;2u"),
        b"\x1b[13;2u",
        TrackedProcessUserInteraction::MayEcho
    )]
    fn test_key_input_interaction_when_key_varies_classifies_prompt_submission(
        #[case] key: ClientKey,
        #[case] bytes: &[u8],
        #[case] expected: TrackedProcessUserInteraction,
    ) {
        pretty_assertions::assert_eq!(self::key_input_interaction(&key, bytes), expected);
    }

    #[rstest::rstest]
    #[case::create_tab(
        ClientKeyCode::Char('E'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bE",
        ClientCmd::Tab(TabCmd::Create)
    )]
    #[case::focus_previous_tab(
        ClientKeyCode::Char('P'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bP",
        ClientCmd::Tab(TabCmd::FocusPrevious)
    )]
    #[case::focus_next_tab(
        ClientKeyCode::Char('N'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bN",
        ClientCmd::Tab(TabCmd::FocusNext)
    )]
    #[case::move_tab_previous(
        ClientKeyCode::Char('p'),
        ClientKeyModifiers::CTRL_ALT,
        b"\x1b\x10",
        ClientCmd::Tab(TabCmd::MovePrevious)
    )]
    #[case::move_tab_next(
        ClientKeyCode::Char('n'),
        ClientKeyModifiers::CTRL_ALT,
        b"\x1b\x0e",
        ClientCmd::Tab(TabCmd::MoveNext)
    )]
    #[case::focus_pane_left(
        ClientKeyCode::Char('H'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bH",
        ClientCmd::FocusPane(PaneFocusDirection::Left)
    )]
    #[case::focus_pane_down(
        ClientKeyCode::Char('J'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bJ",
        ClientCmd::FocusPane(PaneFocusDirection::Down)
    )]
    #[case::focus_pane_up(
        ClientKeyCode::Char('K'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bK",
        ClientCmd::FocusPane(PaneFocusDirection::Up)
    )]
    #[case::focus_pane_right(
        ClientKeyCode::Char('L'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bL",
        ClientCmd::FocusPane(PaneFocusDirection::Right)
    )]
    #[case::split_pane_vertical(
        ClientKeyCode::Char('V'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bV",
        ClientCmd::SplitPane(PaneSplitAxis::Vertical)
    )]
    #[case::split_pane_horizontal(
        ClientKeyCode::Char('D'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bD",
        ClientCmd::SplitPane(PaneSplitAxis::Horizontal)
    )]
    #[case::toggle_pane_fullscreen(
        ClientKeyCode::Char('F'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bF",
        ClientCmd::TogglePaneFullscreen
    )]
    #[case::close_pane(
        ClientKeyCode::Char('W'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bW",
        ClientCmd::ClosePane
    )]
    #[case::open_scrollback_editor(
        ClientKeyCode::Char('S'),
        ClientKeyModifiers::SHIFT_ALT,
        b"\x1bS",
        ClientCmd::OpenScrollbackEditor
    )]
    fn test_resolve_key_when_normal_bound_key_arrives_returns_cmd(
        #[case] code: ClientKeyCode,
        #[case] modifiers: ClientKeyModifiers,
        #[case] raw_bytes: &[u8],
        #[case] cmd: ClientCmd,
    ) {
        let mut input_mode = ServerInputMode::Normal;
        let key = ClientKey {
            code,
            modifiers,
            raw_bytes: raw_bytes.to_vec(),
        };

        pretty_assertions::assert_eq!(self::resolve_key(&mut input_mode, &key), KeyResolution::Cmd(cmd),);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    #[test]
    fn test_resolve_key_when_unbound_key_arrives_returns_raw() {
        let mut input_mode = ServerInputMode::Normal;
        let key = ClientKey {
            code: ClientKeyCode::Char('x'),
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: b"x".to_vec(),
        };

        pretty_assertions::assert_eq!(self::resolve_key(&mut input_mode, &key), KeyResolution::Raw);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    #[rstest::rstest]
    #[case::left(ClientKeyCode::Char('h'), ClientCmd::ResizePane(PaneResizeDirection::Left))]
    #[case::down(ClientKeyCode::Char('j'), ClientCmd::ResizePane(PaneResizeDirection::Down))]
    #[case::up(ClientKeyCode::Char('k'), ClientCmd::ResizePane(PaneResizeDirection::Up))]
    #[case::right(ClientKeyCode::Char('l'), ClientCmd::ResizePane(PaneResizeDirection::Right))]
    #[case::arrow_left(ClientKeyCode::Left, ClientCmd::ResizePane(PaneResizeDirection::Left))]
    #[case::arrow_down(ClientKeyCode::Down, ClientCmd::ResizePane(PaneResizeDirection::Down))]
    #[case::arrow_up(ClientKeyCode::Up, ClientCmd::ResizePane(PaneResizeDirection::Up))]
    #[case::arrow_right(ClientKeyCode::Right, ClientCmd::ResizePane(PaneResizeDirection::Right))]
    fn test_resolve_key_when_resize_mode_key_arrives_returns_resize_cmd(
        #[case] code: ClientKeyCode,
        #[case] cmd: ClientCmd,
    ) {
        let mut input_mode = ServerInputMode::Resize;
        let key = ClientKey {
            code,
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: b"x".to_vec(),
        };

        pretty_assertions::assert_eq!(self::resolve_key(&mut input_mode, &key), KeyResolution::Cmd(cmd),);
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Resize);
    }

    #[test]
    fn test_resolve_key_when_resize_mode_enter_and_exit_arrive_updates_server_mode() {
        let mut input_mode = ServerInputMode::Normal;
        let enter = ClientKey {
            code: ClientKeyCode::Char('R'),
            modifiers: ClientKeyModifiers::SHIFT_ALT,
            raw_bytes: b"\x1bR".to_vec(),
        };
        let exit = ClientKey {
            code: ClientKeyCode::Esc,
            modifiers: ClientKeyModifiers::NONE,
            raw_bytes: b"\x1b".to_vec(),
        };

        pretty_assertions::assert_eq!(
            self::resolve_key(&mut input_mode, &enter),
            KeyResolution::Cmd(ClientCmd::EnterResizeMode),
        );
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Resize);
        pretty_assertions::assert_eq!(
            self::resolve_key(&mut input_mode, &exit),
            KeyResolution::Cmd(ClientCmd::ExitMode),
        );
        pretty_assertions::assert_eq!(input_mode, ServerInputMode::Normal);
    }

    fn key(code: ClientKeyCode, modifiers: ClientKeyModifiers, raw_bytes: &[u8]) -> ClientKey {
        ClientKey {
            code,
            modifiers,
            raw_bytes: raw_bytes.to_vec(),
        }
    }

    const fn modifiers(shift: bool, alt: bool, ctrl: bool) -> ClientKeyModifiers {
        ClientKeyModifiers { alt, ctrl, shift }
    }
}
