use muxr_core::ClientKey;
use muxr_core::ClientKeyCode;
use muxr_core::ClientKeyModifiers;

use crate::pane_borders::BorderRenderMode;
use crate::pane_focus::PaneFocusDirection;
use crate::pane_resize::PaneResizeDirection;
use crate::pane_split::PaneSplitAxis;
use crate::pane_tracked_process::TrackedProcessUserInteraction;

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
}
