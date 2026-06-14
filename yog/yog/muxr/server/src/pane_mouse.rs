use std::time::Instant;

use muxr_core::ClientMouseEvent;
use muxr_core::ClientMouseEventPhase;
use muxr_core::PaneRegionSnapshot;
use muxr_core::PaneScrollDirection;
use rootcause::report;

use crate::client_session::ClientSessionState;
use crate::pane_focus::PaneFocusClientOutcome;
use crate::pane_tracked_process::TrackedProcessUserInteraction;
use crate::terminal::TerminalApplicationMode;
use crate::terminal::TerminalCursorKeyMode;
use crate::terminal::TerminalMouseProtocol;
use crate::terminal::TerminalMouseProtocolEncoding;
use crate::terminal::TerminalScreenMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneMouseAction {
    ForwardToPty {
        focus: PaneMouseFocus,
        // Keep the protocol chosen during action resolution so PTY output parsed before the write cannot reclassify or
        // drop the same mouse event.
        protocol: TerminalMouseProtocol,
    },
    FauxScrollPty {
        cursor_key_mode: TerminalCursorKeyMode,
        direction: PaneScrollDirection,
    },
    FocusPane,
    NoAction,
    ScrollHistory {
        direction: PaneScrollDirection,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PaneMouseFocus {
    FocusPointedPane,
    PreserveFocus,
}

impl PaneMouseFocus {
    const fn focuses_pane(self) -> bool {
        matches!(self, Self::FocusPointedPane)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneMouseClientOutcome {
    pub focus: PaneFocusClientOutcome,
    pub render_dirty: bool,
    pub sync_render_deadline: bool,
}

impl PaneMouseClientOutcome {
    const fn unchanged() -> Self {
        Self {
            focus: PaneFocusClientOutcome::Unchanged,
            render_dirty: false,
            sync_render_deadline: false,
        }
    }
}

pub fn handle_mouse_event_client_request(
    event: ClientMouseEvent,
    state: &mut ClientSessionState<'_>,
) -> rootcause::Result<PaneMouseClientOutcome> {
    let Some(region) = crate::screen_render::visible_pane_region_snapshot_at_position(state, event.position)? else {
        return Ok(PaneMouseClientOutcome::unchanged());
    };
    let handle = state.runtimes.handle(*region.id())?;
    let action = self::resolve_pane_mouse_action(event, handle.application_mode()?);
    match action {
        PaneMouseAction::ForwardToPty { focus, protocol } => {
            // Focus-reporting apps must observe the pane transition before the click bytes; only the layout render can
            // wait until after forwarding the mouse packet.
            let focus = if focus.focuses_pane() {
                crate::pane_focus::handle_focus_pane_at_client_request(event.position, state)?
            } else {
                PaneFocusClientOutcome::Unchanged
            };
            let Some(scrolled_to_bottom) = handle.write_mouse_event(event, &region, protocol)? else {
                return Ok(PaneMouseClientOutcome {
                    focus,
                    render_dirty: false,
                    sync_render_deadline: false,
                });
            };
            state.pane_tracked_processes.record_user_interaction(
                *region.id(),
                TrackedProcessUserInteraction::MayEcho,
                Instant::now(),
            );
            Ok(PaneMouseClientOutcome {
                focus,
                render_dirty: scrolled_to_bottom,
                sync_render_deadline: true,
            })
        }
        PaneMouseAction::FauxScrollPty {
            cursor_key_mode,
            direction,
        } => {
            let render_dirty = handle.write_faux_scroll_input(direction, cursor_key_mode)?;
            state.pane_tracked_processes.record_user_interaction(
                *region.id(),
                TrackedProcessUserInteraction::MayEcho,
                Instant::now(),
            );
            Ok(PaneMouseClientOutcome {
                focus: PaneFocusClientOutcome::Unchanged,
                render_dirty,
                sync_render_deadline: true,
            })
        }
        PaneMouseAction::ScrollHistory { direction } => {
            let outcome =
                crate::pane_scroll::handle_scroll_pane_wheel_client_request(event.position, direction, state)?;
            Ok(PaneMouseClientOutcome {
                focus: PaneFocusClientOutcome::Unchanged,
                render_dirty: outcome.render_dirty,
                sync_render_deadline: outcome.sync_render_deadline,
            })
        }
        PaneMouseAction::FocusPane => Ok(PaneMouseClientOutcome {
            focus: crate::pane_focus::handle_focus_pane_at_client_request(event.position, state)?,
            render_dirty: false,
            sync_render_deadline: false,
        }),
        PaneMouseAction::NoAction => Ok(PaneMouseClientOutcome::unchanged()),
    }
}

fn resolve_pane_mouse_action(event: ClientMouseEvent, mode: TerminalApplicationMode) -> PaneMouseAction {
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
        if mode.screen_mode == TerminalScreenMode::Alternate {
            return PaneMouseAction::FauxScrollPty {
                cursor_key_mode: mode.cursor_key_mode,
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

pub fn encode_pty_mouse_event(
    event: ClientMouseEvent,
    region: &PaneRegionSnapshot,
    protocol: TerminalMouseProtocol,
) -> rootcause::Result<Option<Vec<u8>>> {
    if !protocol.reports_event(event) {
        return Ok(None);
    }

    let Some((row, col)) = self::pane_local_mouse_position(event.position, region) else {
        return Ok(None);
    };
    let row = row.checked_add(1).ok_or_else(|| report!("muxr mouse row overflowed"))?;
    let col = col
        .checked_add(1)
        .ok_or_else(|| report!("muxr mouse column overflowed"))?;

    match protocol.encoding {
        TerminalMouseProtocolEncoding::Sgr => Ok(Some(self::sgr_mouse_event_bytes(event, row, col))),
        TerminalMouseProtocolEncoding::Default => Ok(self::default_mouse_event_bytes(event, row, col)),
        TerminalMouseProtocolEncoding::Utf8 => Ok(self::utf8_mouse_event_bytes(event, row, col)),
    }
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

fn pane_local_mouse_position(
    position: muxr_core::ClientMousePosition,
    region: &PaneRegionSnapshot,
) -> Option<(u16, u16)> {
    if !region.contains(position.row, position.col) {
        return None;
    }
    Some((
        position.row.checked_sub(region.row())?,
        position.col.checked_sub(region.col())?,
    ))
}

fn sgr_mouse_event_bytes(event: ClientMouseEvent, row: u16, col: u16) -> Vec<u8> {
    let final_byte = match event.phase {
        ClientMouseEventPhase::Press => "M",
        ClientMouseEventPhase::Release => "m",
    };
    format!("\x1b[<{};{col};{row}{final_byte}", event.button).into_bytes()
}

fn default_mouse_event_bytes(event: ClientMouseEvent, row: u16, col: u16) -> Option<Vec<u8>> {
    let button = if event.phase == ClientMouseEventPhase::Release {
        (event.button & !0b11) | 0b11
    } else {
        event.button
    };
    let button = u8::try_from(button.checked_add(32)?).ok()?;
    let col = u8::try_from(col.checked_add(32)?).ok()?;
    let row = u8::try_from(row.checked_add(32)?).ok()?;

    Some(vec![0x1b, b'[', b'M', button, col, row])
}

fn utf8_mouse_event_bytes(event: ClientMouseEvent, row: u16, col: u16) -> Option<Vec<u8>> {
    let button = if event.phase == ClientMouseEventPhase::Release {
        (event.button & !0b11) | 0b11
    } else {
        event.button
    };
    let mut bytes = b"\x1b[M".to_vec();
    self::push_utf8_mouse_value(&mut bytes, button.checked_add(32)?)?;
    self::push_utf8_mouse_value(&mut bytes, col.checked_add(32)?)?;
    self::push_utf8_mouse_value(&mut bytes, row.checked_add(32)?)?;
    Some(bytes)
}

fn push_utf8_mouse_value(bytes: &mut Vec<u8>, value: u16) -> Option<()> {
    let ch = char::from_u32(u32::from(value))?;
    let mut encoded = [0; 4];
    bytes.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use muxr_core::ClientMousePosition;
    use muxr_core::PaneMouseMode;
    use rstest::rstest;

    use super::*;
    use crate::terminal::TerminalMouseProtocol;
    use crate::terminal::TerminalMouseProtocolEncoding;
    use crate::terminal::TerminalMouseProtocolMode;

    #[test]
    fn test_encode_pty_mouse_event_when_sgr_mouse_is_enabled_translates_to_pane_local_position() -> rootcause::Result<()>
    {
        let event = ClientMouseEvent {
            button: 0,
            phase: ClientMouseEventPhase::Press,
            position: ClientMousePosition { row: 4, col: 7 },
        };
        let region = PaneRegionSnapshot::new(muxr_core::PaneId::new(1)?, 5, 3, 10, 4, PaneMouseMode::ButtonMotion, 0)?;

        pretty_assertions::assert_eq!(
            encode_pty_mouse_event(
                event,
                &region,
                TerminalMouseProtocol {
                    mode: TerminalMouseProtocolMode::PressRelease,
                    encoding: TerminalMouseProtocolEncoding::Sgr
                },
            )?,
            Some(b"\x1b[<0;3;2M".to_vec()),
        );
        Ok(())
    }

    #[test]
    fn test_encode_pty_mouse_event_when_protocol_ignores_motion_returns_none() -> rootcause::Result<()> {
        let event = ClientMouseEvent {
            button: 32,
            phase: ClientMouseEventPhase::Press,
            position: ClientMousePosition { row: 4, col: 7 },
        };
        let region = PaneRegionSnapshot::new(muxr_core::PaneId::new(1)?, 5, 3, 10, 4, PaneMouseMode::ButtonMotion, 0)?;

        pretty_assertions::assert_eq!(
            encode_pty_mouse_event(
                event,
                &region,
                TerminalMouseProtocol {
                    mode: TerminalMouseProtocolMode::Press,
                    encoding: TerminalMouseProtocolEncoding::Sgr
                },
            )?,
            None,
        );
        Ok(())
    }

    #[test]
    fn test_encode_pty_mouse_event_when_button_motion_gets_no_button_motion_returns_none() -> rootcause::Result<()> {
        let event = ClientMouseEvent {
            button: 35,
            phase: ClientMouseEventPhase::Press,
            position: ClientMousePosition { row: 4, col: 7 },
        };
        let region = PaneRegionSnapshot::new(muxr_core::PaneId::new(1)?, 5, 3, 10, 4, PaneMouseMode::ButtonMotion, 0)?;

        pretty_assertions::assert_eq!(
            encode_pty_mouse_event(
                event,
                &region,
                TerminalMouseProtocol {
                    mode: TerminalMouseProtocolMode::ButtonMotion,
                    encoding: TerminalMouseProtocolEncoding::Sgr
                },
            )?,
            None,
        );
        Ok(())
    }

    #[test]
    fn test_encode_pty_mouse_event_when_any_motion_gets_no_button_motion_reports_event() -> rootcause::Result<()> {
        let event = ClientMouseEvent {
            button: 35,
            phase: ClientMouseEventPhase::Press,
            position: ClientMousePosition { row: 4, col: 7 },
        };
        let region = PaneRegionSnapshot::new(muxr_core::PaneId::new(1)?, 5, 3, 10, 4, PaneMouseMode::AnyMotion, 0)?;

        pretty_assertions::assert_eq!(
            encode_pty_mouse_event(
                event,
                &region,
                TerminalMouseProtocol {
                    mode: TerminalMouseProtocolMode::AnyMotion,
                    encoding: TerminalMouseProtocolEncoding::Sgr
                },
            )?,
            Some(b"\x1b[<35;3;2M".to_vec()),
        );
        Ok(())
    }

    #[test]
    fn test_encode_pty_mouse_event_when_utf8_mouse_is_enabled_writes_utf8_values() -> rootcause::Result<()> {
        let event = ClientMouseEvent {
            button: 0,
            phase: ClientMouseEventPhase::Press,
            position: ClientMousePosition { row: 4, col: 7 },
        };
        let region = PaneRegionSnapshot::new(muxr_core::PaneId::new(1)?, 5, 3, 10, 4, PaneMouseMode::ButtonMotion, 0)?;

        pretty_assertions::assert_eq!(
            encode_pty_mouse_event(
                event,
                &region,
                TerminalMouseProtocol {
                    mode: TerminalMouseProtocolMode::PressRelease,
                    encoding: TerminalMouseProtocolEncoding::Utf8
                },
            )?,
            Some(b"\x1b[M #\"".to_vec()),
        );
        Ok(())
    }

    #[rstest]
    #[case::wheel_up(64)]
    #[case::wheel_down(65)]
    fn test_resolve_pane_mouse_action_when_wheel_is_reported_forwards_to_pty(#[case] button: u16) {
        let event = self::mouse_press(button);
        let protocol = TerminalMouseProtocol {
            encoding: TerminalMouseProtocolEncoding::Sgr,
            mode: TerminalMouseProtocolMode::Press,
        };
        let mode = self::application_mode(
            TerminalScreenMode::Normal,
            TerminalCursorKeyMode::Normal,
            Some(protocol),
        );

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
            resolve_pane_mouse_action(
                event,
                self::application_mode(TerminalScreenMode::Alternate, TerminalCursorKeyMode::Application, None),
            ),
            PaneMouseAction::FauxScrollPty {
                cursor_key_mode: TerminalCursorKeyMode::Application,
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
            resolve_pane_mouse_action(
                event,
                self::application_mode(TerminalScreenMode::Normal, TerminalCursorKeyMode::Normal, None),
            ),
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
        let mode = self::application_mode(
            TerminalScreenMode::Normal,
            TerminalCursorKeyMode::Normal,
            Some(protocol),
        );

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
            resolve_pane_mouse_action(
                event,
                self::application_mode(TerminalScreenMode::Normal, TerminalCursorKeyMode::Normal, None),
            ),
            PaneMouseAction::FocusPane,
        );
    }

    #[test]
    fn test_resolve_pane_mouse_action_when_motion_is_not_reported_has_no_action() {
        let event = self::mouse_press(32);

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(
                event,
                self::application_mode(TerminalScreenMode::Normal, TerminalCursorKeyMode::Normal, None),
            ),
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
        let mode = self::application_mode(
            TerminalScreenMode::Normal,
            TerminalCursorKeyMode::Normal,
            Some(protocol),
        );

        pretty_assertions::assert_eq!(
            resolve_pane_mouse_action(event, mode),
            PaneMouseAction::ForwardToPty {
                focus: PaneMouseFocus::PreserveFocus,
                protocol,
            },
        );
    }

    fn application_mode(
        screen_mode: TerminalScreenMode,
        cursor_key_mode: TerminalCursorKeyMode,
        mouse_protocol: Option<TerminalMouseProtocol>,
    ) -> TerminalApplicationMode {
        TerminalApplicationMode {
            screen_mode,
            cursor_key_mode,
            focus_reporting: crate::terminal::TerminalFocusReporting::Disabled,
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
