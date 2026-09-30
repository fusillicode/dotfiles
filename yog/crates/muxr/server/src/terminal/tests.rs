use std::fmt::Write as _;

use muxr_config::MuxrConfig;
use muxr_core::RenderCell;
use muxr_core::RenderCellWidth;
use muxr_core::RenderCursorShape;
use rootcause::report;
use test_that::prelude::*;

use super::*;

mod scrollback;

mod snapshot;

fn assert_replies_eq(replies: &TerminalReplies, expected: &[Vec<u8>]) {
    test_that::assert_that!(replies.as_ref(), eq(expected));
}

#[test]
fn test_paste_input_bytes_when_bracketed_paste_is_enabled_wraps_payload() {
    test_that::assert_that!(
        paste_input_bytes(b"one\ntwo\n", TerminalPasteMode::Bracketed),
        eq(b"\x1b[200~one\ntwo\n\x1b[201~".to_vec())
    );
}

#[test]
fn test_paste_input_bytes_when_bracketed_paste_is_disabled_preserves_payload() {
    test_that::assert_that!(
        paste_input_bytes(b"one\ntwo\n", TerminalPasteMode::Plain),
        eq(b"one\ntwo\n".to_vec())
    );
}

#[rstest::rstest]
#[case::status_report(b"\x1b[5n", b"\x1b[0n")]
#[case::cursor_report(b"\x1b[6n", b"\x1b[1;1R")]
fn test_terminal_state_process_when_terminal_report_requested_returns_reply(
    #[case] bytes: &[u8],
    #[case] expected: &[u8],
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(bytes).into_replies(), &[expected.to_vec()]);
    Ok(())
}

#[test]
fn test_terminal_state_process_when_cursor_report_requested_returns_current_cursor() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b[2;3H").into_replies(), &[]);

    self::assert_replies_eq(&terminal.process(b"\x1b[6n").into_replies(), &[b"\x1b[2;3R".to_vec()]);
    Ok(())
}

#[test]
fn test_terminal_state_process_when_only_cursor_shape_changes_marks_screen_dirty() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let outcome = terminal.process(b"\x1b[6 q");

    test_that::assert_that!(outcome.screen_dmg(), eq(TerminalScreenDmg::Dirty));
    Ok(())
}

#[rstest::rstest]
#[case::cursor_visibility(b"\x1b[?25l")]
#[case::mouse_protocol(b"\x1b[?1002h\x1b[?1006h")]
fn test_terminal_state_process_when_only_render_metadata_changes_marks_render_dirty(
    #[case] bytes: &[u8],
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let outcome = terminal.process(bytes);

    test_that::assert_that!(outcome.screen_dmg(), eq(TerminalScreenDmg::Dirty));
    Ok(())
}

#[test]
fn test_terminal_state_process_when_utf8_contains_c1_byte_preserves_text_and_screen_mode() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _outcome = terminal.process("ś?47h".as_bytes());

    test_that::assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        contains_substring("ś?47h")
    );
    Ok(())
}

#[test]
fn test_terminal_state_process_when_utf8_c1_byte_is_split_preserves_text_and_screen_mode() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _first = terminal.process(b"\xc5");
    let _second = terminal.process(b"\x9b?47h");

    test_that::assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        contains_substring("ś?47h")
    );
    Ok(())
}

#[test]
fn test_terminal_state_process_when_report_sequence_is_split_returns_one_reply() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b[").into_replies(), &[]);

    self::assert_replies_eq(&terminal.process(b"6n").into_replies(), &[b"\x1b[1;1R".to_vec()]);
    Ok(())
}

#[rstest::rstest]
#[case::osc_zero(b"\x1b]0;cargo test\x07")]
#[case::osc_two(b"\x1b]2;cargo test\x07")]
fn test_terminal_state_title_when_window_title_is_set_returns_title(#[case] bytes: &[u8]) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(bytes).into_replies(), &[]);

    test_that::assert_that!(terminal.title(), eq(Some("cargo test".to_owned())));
    Ok(())
}

#[test]
fn test_terminal_state_take_title_changes_when_window_title_changes_returns_once() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b]2;cargo test\x07").into_replies(), &[]);

    test_that::assert_that!(terminal.take_title_changes(), eq(vec![Some("cargo test".to_owned())]));
    test_that::assert_that!(terminal.take_title_changes(), eq(Vec::<Option<String>>::new()));
    Ok(())
}

#[test]
fn test_terminal_state_take_title_changes_when_window_title_repeats_returns_empty() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b]2;cargo test\x07").into_replies(), &[]);
    test_that::assert_that!(terminal.take_title_changes(), eq(vec![Some("cargo test".to_owned())]));
    self::assert_replies_eq(&terminal.process(b"\x1b]2;cargo test\x07").into_replies(), &[]);

    test_that::assert_that!(terminal.take_title_changes(), eq(Vec::<Option<String>>::new()));
    Ok(())
}

#[test]
fn test_terminal_state_take_title_changes_when_titles_change_in_one_chunk_preserves_order() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b]2;gst\x07\x1b]2;~\x07").into_replies(), &[]);

    test_that::assert_that!(
        terminal.take_title_changes(),
        eq(vec![Some("gst".to_owned()), Some("~".to_owned())])
    );
    Ok(())
}

#[test]
fn test_terminal_state_title_when_window_title_sequence_is_split_returns_title() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b]2;").into_replies(), &[]);
    self::assert_replies_eq(&terminal.process(b"gst\x07").into_replies(), &[]);

    test_that::assert_that!(terminal.title(), eq(Some("gst".to_owned())));
    Ok(())
}

#[rstest::rstest]
#[case::can(b'\x18')]
#[case::sub(b'\x1a')]
fn test_terminal_state_title_when_split_window_title_is_canceled_remains_unchanged(
    #[case] cancel: u8,
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);
    let _ = terminal.process(b"\x1b]2;before\x07");
    let _changes = terminal.take_title_changes();

    let _ = terminal.process(b"\x1b]2;after");
    let _ = terminal.process(&[cancel]);

    test_that::assert_that!(terminal.title(), eq(Some("before".to_owned())));
    test_that::assert_that!(terminal.take_title_changes(), eq(Vec::<Option<String>>::new()));
    Ok(())
}

#[test]
fn test_terminal_state_title_when_window_title_is_empty_returns_none() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(b"\x1b]2;cargo test\x07");
    let _ = terminal.process(b"\x1b]2;  \x07");

    test_that::assert_that!(terminal.title(), eq(None));
    Ok(())
}

#[rstest::rstest]
#[case::osc_zero_bel(b"\x1b]0;cargo test\x07")]
#[case::osc_two_st(b"\x1b]2;cargo test\x1b\\")]
#[case::multiple_titles(b"\x1b]2;gst\x07\x1b]2;~\x07")]
fn test_terminal_state_process_when_only_title_changes_keeps_screen_clean(
    #[case] bytes: &[u8],
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let outcome = terminal.process(bytes);

    test_that::assert_that!(outcome.screen_dmg(), eq(TerminalScreenDmg::Clean));
    self::assert_replies_eq(&outcome.into_replies(), &[]);
    Ok(())
}

#[test]
fn test_terminal_state_process_when_title_sequence_is_split_keeps_screen_clean() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let first = terminal.process(b"\x1b]2;");
    let second = terminal.process(b"gst\x07");

    test_that::assert_that!(first.screen_dmg(), eq(TerminalScreenDmg::Clean));
    self::assert_replies_eq(&first.into_replies(), &[]);
    test_that::assert_that!(second.screen_dmg(), eq(TerminalScreenDmg::Clean));
    self::assert_replies_eq(&second.into_replies(), &[]);
    test_that::assert_that!(terminal.title(), eq(Some("gst".to_owned())));
    Ok(())
}

#[rstest::rstest]
#[case::text(b"hi")]
#[case::title_then_text(b"\x1b]2;gst\x07hi")]
#[case::canceled_title_then_text(b"\x1b]2;gst\x18hi")]
fn test_terminal_state_process_when_output_is_not_title_only_marks_screen_dirty(
    #[case] bytes: &[u8],
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    test_that::assert_that!(terminal.process(bytes).screen_dmg(), eq(TerminalScreenDmg::Dirty));
    Ok(())
}

#[test]
fn test_terminal_state_bracketed_paste_when_mode_is_enabled_returns_true() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(b"\x1b[?2004h");

    test_that::assert_that!(terminal.paste_mode(), eq(TerminalPasteMode::Bracketed));
    Ok(())
}

#[test]
fn test_terminal_state_mouse_protocol_when_sgr_button_motion_is_enabled_returns_protocol() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(b"\x1b[?1002h\x1b[?1006h");

    test_that::assert_that!(
        terminal.mouse_protocol(),
        eq(Some(TerminalMouseProtocol {
            mode: TerminalMouseProtocolMode::ButtonMotion,
            encoding: TerminalMouseProtocolEncoding::Sgr
        }))
    );
    test_that::assert_that!(
        terminal.application_mode().pane_mouse_mode(),
        eq(PaneMouseMode::ButtonMotion)
    );
    test_that::assert_that!(
        terminal.application_mode().pane_mouse_mode(),
        eq(PaneMouseMode::ButtonMotion)
    );
    Ok(())
}

#[rstest::rstest]
#[case::alternate_47_enabled(b"\x1b[?47h", TerminalScreenMode::Alternate)]
#[case::alternate_1049_enabled(b"\x1b[?1049h", TerminalScreenMode::Alternate)]
#[case::alternate_47_disabled(b"\x1b[?47h\x1b[?47l", TerminalScreenMode::Normal)]
#[case::alternate_1049_disabled(b"\x1b[?1049h\x1b[?1049l", TerminalScreenMode::Normal)]
fn test_terminal_state_application_mode_when_alternate_screen_changes_returns_state(
    #[case] bytes: &[u8],
    #[case] expected: TerminalScreenMode,
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(bytes);

    test_that::assert_that!(
        terminal.application_mode(),
        eq(TerminalApplicationMode {
            screen_mode: expected,
            cursor_key_mode: TerminalCursorKeyMode::Normal,
            keyboard_protocol: TerminalKeyboardProtocol::Legacy,
            focus_reporting: TerminalFocusReporting::Disabled,
            mouse_protocol: None,
        })
    );
    Ok(())
}

#[test]
fn test_terminal_state_application_mode_when_legacy_alternate_screen_sequence_is_split_returns_state()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(b"\x1b[?4");
    test_that::assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
    let _ = terminal.process(b"7h");
    test_that::assert_that!(
        terminal.application_mode().screen_mode,
        eq(TerminalScreenMode::Alternate)
    );
    let _ = terminal.process(b"\x1b[?47");
    let _ = terminal.process(b"l");

    test_that::assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
    Ok(())
}

#[rstest::rstest]
#[case::bell(b"\x07")]
#[case::delete(b"\x7f")]
fn test_terminal_state_application_mode_when_csi_control_is_embedded_keeps_parsing_legacy_alternate_screen(
    #[case] control: &[u8],
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);
    let mut sequence = b"\x1b[?4".to_vec();
    sequence.extend_from_slice(control);
    sequence.extend_from_slice(b"7h");

    let _outcome = terminal.process(&sequence);

    test_that::assert_that!(
        terminal.application_mode().screen_mode,
        eq(TerminalScreenMode::Alternate)
    );
    Ok(())
}

#[test]
fn test_terminal_state_application_mode_when_csi_control_precedes_chunk_boundary_keeps_parsing_legacy_alternate_screen()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _first = terminal.process(b"\x1b[?4\x07");
    let _second = terminal.process(b"7h");

    test_that::assert_that!(
        terminal.application_mode().screen_mode,
        eq(TerminalScreenMode::Alternate)
    );
    Ok(())
}

#[test]
fn test_terminal_state_application_mode_when_legacy_alternate_screen_is_grouped_returns_state() -> rootcause::Result<()>
{
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _enabled = terminal.process(b"\x1b[?1;47h");
    test_that::assert_that!(
        terminal.application_mode().screen_mode,
        eq(TerminalScreenMode::Alternate)
    );
    test_that::assert_that!(
        terminal.application_mode().cursor_key_mode,
        eq(TerminalCursorKeyMode::Application)
    );

    let _disabled = terminal.process(b"\x1b[?1;47l");
    test_that::assert_that!(terminal.application_mode().screen_mode, eq(TerminalScreenMode::Normal));
    test_that::assert_that!(
        terminal.application_mode().cursor_key_mode,
        eq(TerminalCursorKeyMode::Normal)
    );
    Ok(())
}

#[test]
fn test_terminal_state_process_when_legacy_alternate_screen_wraps_content_keeps_grids_separate() -> rootcause::Result<()>
{
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);

    let _ = terminal.process(b"normal\x1b[?47halt");

    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        contains_substring("alt")
    );
    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        not(contains_substring("normal"))
    );

    let _ = terminal.process(b"\x1b[?47l");

    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        contains_substring("normal")
    );
    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        not(contains_substring("alt"))
    );
    Ok(())
}

#[test]
fn test_terminal_state_process_when_legacy_alternate_screen_is_reentered_preserves_contents() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);

    let _ = terminal.process(b"normal\x1b[?47halt\x1b[?47l\x1b[?47h");

    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        contains_substring("alt")
    );
    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        not(contains_substring("normal"))
    );
    Ok(())
}

#[test]
fn test_terminal_state_process_when_reset_precedes_legacy_alternate_reentry_does_not_restore_stale_contents()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);
    let _setup = terminal.process(b"normal\x1b[?47hlegacy\x1b[?47l");

    let _reset_and_reenter = terminal.process(b"\x1bc\x1b[?47h");

    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        not(contains_substring("legacy"))
    );
    Ok(())
}

#[test]
fn test_terminal_state_process_when_native_alternate_cycle_is_buffered_does_not_restore_older_legacy_contents()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);
    let _legacy = terminal.process(b"normal\x1b[?47hlegacy\x1b[?47l");

    let _native_cycle = terminal.process(b"\x1b[?1049hnative\x1b[?1049l");
    let _legacy_reentry = terminal.process(b"\x1b[?47h");

    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        not(contains_substring("legacy"))
    );
    Ok(())
}

#[test]
fn test_terminal_state_process_when_c1_legacy_alternate_screen_is_used_keeps_grids_separate() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);

    let _alternate = terminal.process(b"normal\x9b?47halt");

    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        contains_substring("alt")
    );
    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        not(contains_substring("normal"))
    );
    Ok(())
}

#[test]
fn test_terminal_state_process_when_c1_native_alternate_cycle_is_buffered_does_not_restore_older_legacy_contents()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 2)?);
    let _legacy = terminal.process(b"normal\x1b[?47hlegacy\x1b[?47l");

    let _native_cycle = terminal.process(b"\x9b?1049hnative\x9b?1049l");
    let _legacy_reentry = terminal.process(b"\x1b[?47h");

    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        not(contains_substring("legacy"))
    );
    Ok(())
}

#[rstest::rstest]
#[case::application_cursor_enabled(b"\x1b[?1h", TerminalCursorKeyMode::Application)]
#[case::application_cursor_disabled(b"\x1b[?1h\x1b[?1l", TerminalCursorKeyMode::Normal)]
fn test_terminal_state_application_mode_when_application_cursor_changes_returns_state(
    #[case] bytes: &[u8],
    #[case] expected: TerminalCursorKeyMode,
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(bytes);

    test_that::assert_that!(
        terminal.application_mode(),
        eq(TerminalApplicationMode {
            screen_mode: TerminalScreenMode::Normal,
            cursor_key_mode: expected,
            keyboard_protocol: TerminalKeyboardProtocol::Legacy,
            focus_reporting: TerminalFocusReporting::Disabled,
            mouse_protocol: None,
        })
    );
    Ok(())
}

#[rstest::rstest]
#[case::enabled(b"\x1b[?1004h", TerminalFocusReporting::Enabled)]
#[case::disabled(b"\x1b[?1004h\x1b[?1004l", TerminalFocusReporting::Disabled)]
#[case::disabled_by_terminal_reset(b"\x1b[?1004h\x1bc", TerminalFocusReporting::Disabled)]
#[case::enabled_after_terminal_reset(b"\x1b[?1004h\x1bc\x1b[?1004h", TerminalFocusReporting::Enabled)]
#[case::enabled_with_other_private_modes(b"\x1b[?1;1004h", TerminalFocusReporting::Enabled)]
fn test_terminal_state_application_mode_when_focus_reporting_changes_returns_state(
    #[case] bytes: &[u8],
    #[case] expected: TerminalFocusReporting,
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(bytes);

    test_that::assert_that!(
        terminal.application_mode(),
        eq(TerminalApplicationMode {
            screen_mode: TerminalScreenMode::Normal,
            cursor_key_mode: if bytes == b"\x1b[?1;1004h" {
                TerminalCursorKeyMode::Application
            } else {
                TerminalCursorKeyMode::Normal
            },
            keyboard_protocol: TerminalKeyboardProtocol::Legacy,
            focus_reporting: expected,
            mouse_protocol: None,
        })
    );
    Ok(())
}

#[rstest::rstest]
#[case::enabled_by_push(b"\x1b[>1u", TerminalKeyboardProtocol::KittyLevelOne)]
#[case::disabled_by_push_zero(b"\x1b[>1u\x1b[>0u", TerminalKeyboardProtocol::Legacy)]
#[case::disabled_by_pop(b"\x1b[>1u\x1b[<u", TerminalKeyboardProtocol::Legacy)]
#[case::enabled_by_set(b"\x1b[=1u", TerminalKeyboardProtocol::KittyLevelOne)]
#[case::disabled_by_set_zero(b"\x1b[=1u\x1b[=0u", TerminalKeyboardProtocol::Legacy)]
#[case::disabled_by_set_replace_without_disambiguate_bit(b"\x1b[=2u", TerminalKeyboardProtocol::Legacy)]
#[case::disabled_by_set_difference(b"\x1b[>1u\x1b[=1;3u", TerminalKeyboardProtocol::Legacy)]
#[case::disabled_by_terminal_reset(b"\x1b[>1u\x1bc", TerminalKeyboardProtocol::Legacy)]
#[case::disabled_by_terminal_reset_clears_keyboard_protocol(b"\x1b[>1u\x1bc\x1b[<u", TerminalKeyboardProtocol::Legacy)]
fn test_terminal_state_application_mode_when_keyboard_protocol_changes_returns_state(
    #[case] bytes: &[u8],
    #[case] expected: TerminalKeyboardProtocol,
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(bytes);

    test_that::assert_that!(terminal.application_mode().keyboard_protocol, eq(expected));
    Ok(())
}

#[test]
fn test_terminal_state_process_when_keyboard_protocol_is_queried_returns_status() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b[?u").into_replies(), &[b"\x1b[?0u".to_vec()]);
    let _ = terminal.process(b"\x1b[>1u");
    self::assert_replies_eq(&terminal.process(b"\x1b[?u").into_replies(), &[b"\x1b[?1u".to_vec()]);
    let _ = terminal.process(b"\x1b[<u");
    self::assert_replies_eq(&terminal.process(b"\x1b[?u").into_replies(), &[b"\x1b[?0u".to_vec()]);
    Ok(())
}

#[test]
fn test_terminal_state_application_mode_when_terminal_reset_sequence_is_split_clears_focus_reporting()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(b"\x1b[?1004h\x1b");
    let _ = terminal.process(b"c");

    test_that::assert_that!(
        terminal.application_mode().focus_reporting,
        eq(TerminalFocusReporting::Disabled)
    );
    Ok(())
}

#[test]
fn test_terminal_state_application_mode_when_mouse_protocol_is_enabled_returns_protocol() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _ = terminal.process(b"\x1b[?1002h\x1b[?1006h");

    test_that::assert_that!(
        terminal.application_mode(),
        eq(TerminalApplicationMode {
            screen_mode: TerminalScreenMode::Normal,
            cursor_key_mode: TerminalCursorKeyMode::Normal,
            keyboard_protocol: TerminalKeyboardProtocol::Legacy,
            focus_reporting: TerminalFocusReporting::Disabled,
            mouse_protocol: Some(TerminalMouseProtocol {
                mode: TerminalMouseProtocolMode::ButtonMotion,
                encoding: TerminalMouseProtocolEncoding::Sgr,
            }),
        })
    );
    Ok(())
}

#[rstest::rstest]
#[case::private_cursor_report(b"\x1b[?6n")]
#[case::unknown_report(b"\x1b[9n")]
fn test_terminal_state_process_when_report_is_unsupported_returns_no_reply(
    #[case] bytes: &[u8],
) -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(bytes).into_replies(), &[]);
    Ok(())
}

#[test]
fn test_terminal_state_process_when_multi_param_status_report_requested_returns_rio_reply() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b[5;6n").into_replies(), &[b"\x1b[0n".to_vec()]);
    Ok(())
}

fn terminal_size() -> rootcause::Result<TerminalSize> {
    TerminalSize::new(8, 4)
}

fn terminal_state(size: &TerminalSize) -> TerminalState {
    TerminalState::with_scrollback(size, MuxrConfig::new().unwrap().scrollback)
}

fn test_scrollback_dump(terminal: &mut TerminalState, style: ScrollbackDumpStyle) -> Vec<u8> {
    terminal.scrollback_dump(style)
}

fn snapshot_text(snapshot: &TerminalSnapshot) -> String {
    snapshot
        .rows()
        .iter()
        .flat_map(|row| row.cells().iter().map(RenderCell::text))
        .collect()
}
