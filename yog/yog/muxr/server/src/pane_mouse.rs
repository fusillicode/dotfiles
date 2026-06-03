use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::PaneScrollDirection;

use crate::terminal::TerminalApplicationMode;
use crate::terminal::TerminalMouseProtocol;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneMouseAction {
    ForwardToPty {
        focus: PaneMouseFocus,
        // Keep the protocol chosen during action resolution so PTY output parsed before the write cannot reclassify or
        // drop the same mouse event.
        protocol: TerminalMouseProtocol,
    },
    FauxScrollPty {
        application_cursor: bool,
        direction: PaneScrollDirection,
    },
    FocusPane,
    NoAction,
    ScrollHistory {
        direction: PaneScrollDirection,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneMouseFocus {
    FocusPointedPane,
    PreserveFocus,
}

impl PaneMouseFocus {
    pub const fn focuses_pane(self) -> bool {
        matches!(self, Self::FocusPointedPane)
    }
}

pub fn resolve_pane_mouse_action(event: ClientMouseEvent, mode: TerminalApplicationMode) -> PaneMouseAction {
    let focus = if self::mouse_event_focuses_pane(event) {
        PaneMouseFocus::FocusPointedPane
    } else {
        PaneMouseFocus::PreserveFocus
    };

    if let Some(direction) = self::wheel_direction(event) {
        if let Some(protocol) = mode.mouse_protocol
            && protocol.reports_event(event)
        {
            return PaneMouseAction::ForwardToPty {
                focus: PaneMouseFocus::PreserveFocus,
                protocol,
            };
        }
        if mode.alternate_screen {
            return PaneMouseAction::FauxScrollPty {
                application_cursor: mode.application_cursor,
                direction,
            };
        }
        return PaneMouseAction::ScrollHistory { direction };
    }

    if let Some(protocol) = mode.mouse_protocol
        && protocol.reports_event(event)
    {
        return PaneMouseAction::ForwardToPty { focus, protocol };
    }

    if focus.focuses_pane() {
        return PaneMouseAction::FocusPane;
    }

    PaneMouseAction::NoAction
}

fn mouse_event_focuses_pane(event: ClientMouseEvent) -> bool {
    event.phase == ClientMouseEventPhase::Press && event.button & (32 | 64) == 0 && event.button & 0b11 != 0b11
}

const fn wheel_direction(event: ClientMouseEvent) -> Option<PaneScrollDirection> {
    if event.button & 64 == 0 {
        return None;
    }

    match event.button & 0b11 {
        0 => Some(PaneScrollDirection::Up),
        1 => Some(PaneScrollDirection::Down),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use muxr_core::ClientMousePosition;
    use rstest::rstest;

    use super::*;
    use crate::terminal::TerminalMouseProtocol;
    use crate::terminal::TerminalMouseProtocolEncoding;
    use crate::terminal::TerminalMouseProtocolMode;

    #[rstest]
    #[case::wheel_up(64)]
    #[case::wheel_down(65)]
    fn test_resolve_pane_mouse_action_when_wheel_is_reported_forwards_to_pty(#[case] button: u16) {
        let event = self::mouse_press(button);
        let protocol = TerminalMouseProtocol {
            encoding: TerminalMouseProtocolEncoding::Sgr,
            mode: TerminalMouseProtocolMode::Press,
        };
        let mode = self::application_mode(false, false, Some(protocol));

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(event, mode),
            PaneMouseAction::ForwardToPty {
                focus: PaneMouseFocus::PreserveFocus,
                protocol,
            },
        );
    }

    #[rstest]
    #[case::wheel_up(64, PaneScrollDirection::Up)]
    #[case::wheel_down(65, PaneScrollDirection::Down)]
    fn test_resolve_pane_mouse_action_when_alternate_screen_without_mouse_protocol_faux_scrolls_pty(
        #[case] button: u16,
        #[case] direction: PaneScrollDirection,
    ) {
        let event = self::mouse_press(button);

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(event, self::application_mode(true, true, None)),
            PaneMouseAction::FauxScrollPty {
                application_cursor: true,
                direction,
            },
        );
    }

    #[rstest]
    #[case::wheel_up(64, PaneScrollDirection::Up)]
    #[case::wheel_down(65, PaneScrollDirection::Down)]
    fn test_resolve_pane_mouse_action_when_plain_pane_receives_wheel_scrolls_history(
        #[case] button: u16,
        #[case] direction: PaneScrollDirection,
    ) {
        let event = self::mouse_press(button);

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(event, self::application_mode(false, false, None)),
            PaneMouseAction::ScrollHistory { direction },
        );
    }

    #[test]
    fn test_resolve_pane_mouse_action_when_click_is_reported_forwards_to_pty_and_focuses() {
        let event = self::mouse_press(0);
        let protocol = TerminalMouseProtocol {
            encoding: TerminalMouseProtocolEncoding::Sgr,
            mode: TerminalMouseProtocolMode::Press,
        };
        let mode = self::application_mode(false, false, Some(protocol));

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(event, mode),
            PaneMouseAction::ForwardToPty {
                focus: PaneMouseFocus::FocusPointedPane,
                protocol,
            },
        );
    }

    #[test]
    fn test_resolve_pane_mouse_action_when_click_is_not_reported_focuses_pane() {
        let event = self::mouse_press(0);

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(event, self::application_mode(false, false, None)),
            PaneMouseAction::FocusPane,
        );
    }

    #[test]
    fn test_resolve_pane_mouse_action_when_motion_is_not_reported_has_no_action() {
        let event = self::mouse_press(32);

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(event, self::application_mode(false, false, None)),
            PaneMouseAction::NoAction,
        );
    }

    #[test]
    fn test_resolve_pane_mouse_action_when_button_motion_is_reported_forwards_motion() {
        let event = self::mouse_press(32);
        let protocol = TerminalMouseProtocol {
            encoding: TerminalMouseProtocolEncoding::Sgr,
            mode: TerminalMouseProtocolMode::ButtonMotion,
        };
        let mode = self::application_mode(false, false, Some(protocol));

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(event, mode),
            PaneMouseAction::ForwardToPty {
                focus: PaneMouseFocus::PreserveFocus,
                protocol,
            },
        );
    }

    fn application_mode(
        alternate_screen: bool,
        application_cursor: bool,
        mouse_protocol: Option<TerminalMouseProtocol>,
    ) -> TerminalApplicationMode {
        TerminalApplicationMode {
            alternate_screen,
            application_cursor,
            mouse_protocol,
        }
    }

    const fn mouse_press(button: u16) -> ClientMouseEvent {
        ClientMouseEvent {
            button,
            phase: ClientMouseEventPhase::Press,
            position: ClientMousePosition { row: 1, col: 2 },
        }
    }
}
