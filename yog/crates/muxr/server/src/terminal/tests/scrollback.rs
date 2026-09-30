use super::*;

#[test]
fn test_terminal_state_scroll_when_output_exceeds_viewport_shows_history() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

    let _ = terminal.process(b"one\ntwo\nthree");

    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);
    test_that::assert_that!(rendered, contains_substring("one"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_large_normal_output_finishes_shows_capped_history() -> rootcause::Result<()> {
    let mut scrollback = MuxrConfig::new()?.scrollback;
    scrollback.rows = 6;
    let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(8, 4)?, scrollback);
    let mut output = String::new();
    for row in 0..20 {
        write!(output, "row-{row:02}\r\n").context("failed to format test output")?;
    }

    let _ = terminal.process(output.as_bytes());

    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("row-16"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_layout_pane_grows_before_output_captures_full_history() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
    terminal.resize(&TerminalSize::new(8, 4)?);
    let mut output = String::new();
    for row in 0..20 {
        write!(output, "row-{row:02}\r\n").context("failed to format test output")?;
    }

    let _ = terminal.process(output.as_bytes());
    for _ in 0..20 {
        let _movement = terminal.scroll_one_line(PaneScrollDirection::Up);
    }
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("row-00"));
    test_that::assert_that!(rendered, contains_substring("row-03"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_output_wraps_preserves_all_visual_rows() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"before\r\n");
    let _ = terminal.process(b"|abcdefghij|\r\n|klmnopqrst|\r\npsql> ");

    let mut rendered = Vec::new();
    rendered.push(self::snapshot_text(
        &terminal.render_snapshot(TerminalSnapshotScope::Full)?,
    ));
    while terminal.scroll_one_line(PaneScrollDirection::Up) == TerminalScrollMove::Moved {
        rendered.push(self::snapshot_text(
            &terminal.render_snapshot(TerminalSnapshotScope::Full)?,
        ));
    }
    let rendered = rendered.join("\n");

    test_that::assert_that!(rendered, contains_substring("|abcdefg"));
    test_that::assert_that!(rendered, contains_substring("hij|"));
    test_that::assert_that!(rendered, contains_substring("|klmnopq"));
    test_that::assert_that!(rendered, contains_substring("rst|"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_bottom_right_cell_is_filled_waits_for_next_printable_to_scroll()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(4, 2)?);

    let _ = terminal.process(b"top\r\nabc");
    let _ = terminal.process(b"d");

    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Unchanged)
    );

    let _ = terminal.process(b"e");

    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);
    test_that::assert_that!(rendered, contains_substring("top"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_wide_printable_wraps_preserves_scrolled_row() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(4, 2)?);

    let _ = terminal.process(b"top\r\nabc");
    let _ = terminal.process("字".as_bytes());

    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);
    test_that::assert_that!(rendered, contains_substring("top"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_alternate_screen_sets_partial_region_preserves_normal_scrollback()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 3)?);

    let _ = terminal.process(b"\x1b[?1049h\x1b[2;3r\x1b[r\x1b[?1049l");
    let _ = terminal.process(b"one\r\ntwo\r\nthree\r\nfour");

    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);
    test_that::assert_that!(rendered, contains_substring("one"));
    Ok(())
}

#[test]
fn test_terminal_state_scrollback_dump_when_partial_history_precedes_normal_output_keeps_chronology()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 3)?);

    let _ = terminal.process(b"partial-old\r\npartial-live");
    let _ = terminal.process(b"\x1b[1;2r\x1b[2;1H\x1b[S\x1b[r");
    let _ = terminal.process(b"\x1b[3;1Hnormal-new-1\r\nnormal-new-2\r\nnormal-new-3\r\n");

    let dump = String::from_utf8(self::test_scrollback_dump(
        &mut terminal,
        ScrollbackDumpStyle::PlainText,
    ))?;

    test_that::assert_that!(
        dump.find("partial-old").is_some_and(|partial_index| {
            dump.find("normal-new-1")
                .is_some_and(|normal_index| partial_index < normal_index)
        }),
        eq(true)
    );
    Ok(())
}

#[test]
fn test_terminal_state_scrollback_dump_when_output_exceeds_viewport_returns_history_and_live_rows()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

    let _ = terminal.process(b"one\r\ntwo\r\nthree");

    test_that::assert_that!(
        String::from_utf8(self::test_scrollback_dump(
            &mut terminal,
            ScrollbackDumpStyle::PlainText
        ))?,
        eq("one\ntwo\nthree\n")
    );
    Ok(())
}

#[test]
fn test_terminal_state_scrollback_dump_when_alternate_screen_is_active_includes_normal_history() -> rootcause::Result<()>
{
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
    let _output = terminal.process(b"one\r\ntwo\r\nthree\x1b[?1049halt");
    let before = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let mode_before = terminal.application_mode();

    let dump = String::from_utf8(self::test_scrollback_dump(
        &mut terminal,
        ScrollbackDumpStyle::PlainText,
    ))?;

    test_that::assert_that!(dump.as_str(), contains_substring("one"));
    test_that::assert_that!(dump.as_str(), contains_substring("alt"));
    test_that::assert_that!(terminal.render_snapshot(TerminalSnapshotScope::Full)?, eq(before));
    test_that::assert_that!(terminal.application_mode(), eq(mode_before));
    Ok(())
}

#[test]
fn test_terminal_state_scrollback_dump_when_viewport_is_scrolled_preserves_viewport() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
    let _ = terminal.process(b"one\r\ntwo\r\nthree");
    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let before = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

    let _dump = self::test_scrollback_dump(&mut terminal, ScrollbackDumpStyle::PlainText);
    let after = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

    test_that::assert_that!(after, eq(before));
    Ok(())
}

#[test]
fn test_terminal_state_scrollback_dump_when_top_partial_scroll_region_moves_rows_includes_captured_rows()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

    test_that::assert_that!(
        String::from_utf8(self::test_scrollback_dump(
            &mut terminal,
            ScrollbackDumpStyle::PlainText
        ))?,
        eq("one\ntwo\nthree\n\n\nprompt\n")
    );
    Ok(())
}

#[test]
fn test_terminal_state_scrollback_dump_when_ansi_style_requested_preserves_rendered_style() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

    let _ = terminal.process(b"\x1b[31mred\x1b[0m");

    test_that::assert_that!(
        String::from_utf8(self::test_scrollback_dump(&mut terminal, ScrollbackDumpStyle::Ansi))?,
        eq("\x1b[0;38;5;1mred\x1b[0m\n\n")
    );
    Ok(())
}

#[test]
fn test_terminal_state_scroll_to_bottom_when_scrolled_shows_live_viewport() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

    let _ = terminal.process(b"one\ntwo\nthree");
    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));

    test_that::assert_that!(terminal.scroll_to_bottom(), eq(TerminalScrollMove::Moved));
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("three"));
    test_that::assert_that!(terminal.scroll_to_bottom(), eq(TerminalScrollMove::Unchanged));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_top_partial_scroll_region_moves_rows_preserves_history() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("one"));
    test_that::assert_that!(rendered, contains_substring("two"));
    Ok(())
}

#[test]
fn test_terminal_state_when_partial_rows_exceed_configured_limit_keeps_recent_rows() -> rootcause::Result<()> {
    let mut scrollback = MuxrConfig::new()?.scrollback;
    scrollback.rows = 2;
    let mut terminal = TerminalState::with_scrollback(&TerminalSize::new(8, 4)?, scrollback);

    for row in 0..4 {
        let _ = terminal.process(format!("\x1b[1;1Hrow-{row}\x1b[2;1Hstill\x1b[3;1Hprompt").as_bytes());
        let _ = terminal.process(b"\x1b[1;3r\x1b[1S\x1b[r");
    }

    test_that::assert_that!(terminal.rio.terminal().grid.history_size(), eq(2));
    let retained_text = String::from_utf8(terminal.scrollback_dump(ScrollbackDumpStyle::PlainText))?;
    test_that::assert_that!(retained_text.as_str(), not(contains_substring("row-0")));
    test_that::assert_that!(retained_text.as_str(), not(contains_substring("row-1")));
    test_that::assert_that!(retained_text.as_str(), contains_substring("row-2"));
    test_that::assert_that!(retained_text.as_str(), contains_substring("row-3"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_partial_scroll_sequence_is_split_preserves_history() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;3r\x1b[");
    let _ = terminal.process(b"2S\x1b[r");

    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("one"));
    test_that::assert_that!(rendered, contains_substring("two"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_top_partial_scroll_region_linefeed_moves_rows_prefers_captured_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"old-0\nold-1\nold-2\nold-3\nold-4\n");
    let _ = terminal.process(b"\x1b[1;1Hcod-0\x1b[2;1Hcod-1\x1b[3;1Hcod-2\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;3r\x1b[3;1H\n\x1b[r");

    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("cod-0"));
    test_that::assert_that!(rendered, not(contains_substring("old-")));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_alternate_screen_linefeed_moves_rows_does_not_capture_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;3r\x1b[3;1H\n\x1b[r");

    test_that::assert_that!(
        terminal.scroll(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Unchanged)
    );
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_top_partial_scroll_region_delete_lines_moves_rows_preserves_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;3r\x1b[1;1H\x1b[2M\x1b[r");

    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("one"));
    test_that::assert_that!(rendered, contains_substring("two"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_full_scroll_region_delete_lines_moves_rows_preserves_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;4r\x1b[1;1H\x1b[2M\x1b[r");

    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("one"));
    test_that::assert_that!(rendered, contains_substring("two"));
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_mid_partial_scroll_region_delete_lines_moves_rows_does_not_capture_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;3r\x1b[2;1H\x1b[2M\x1b[r");

    test_that::assert_that!(
        terminal.scroll(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Unchanged)
    );
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_alternate_screen_delete_lines_moves_rows_does_not_capture_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;4r\x1b[1;1H\x1b[2M\x1b[r");

    test_that::assert_that!(
        terminal.scroll(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Unchanged)
    );
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_alternate_screen_partial_scroll_region_moves_rows_does_not_capture_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;3r\x1b[2S\x1b[r");

    test_that::assert_that!(
        terminal.scroll(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Unchanged)
    );
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_alternate_screen_full_scroll_region_moves_rows_does_not_capture_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[?1049h\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;4r\x1b[2S\x1b[r");

    test_that::assert_that!(
        terminal.scroll(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Unchanged)
    );
    Ok(())
}

#[test]
fn test_terminal_state_scroll_when_normal_screen_full_scroll_region_moves_rows_preserves_history()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 4)?);

    let _ = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hthree\x1b[4;1Hprompt");
    let _ = terminal.process(b"\x1b[1;4r\x1b[2S\x1b[r");

    test_that::assert_that!(terminal.rio.terminal().grid.history_size(), eq(2));
    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, contains_substring("one"));
    test_that::assert_that!(rendered, contains_substring("two"));
    Ok(())
}

#[test]
fn test_terminal_state_visible_top_row_when_scrolled_tracks_current_viewport() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);

    let _ = terminal.process(b"one\ntwo\nthree");
    let bottom_top_row = terminal.visible_top_row()?;
    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let scrolled_snapshot = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    let scrolled_top_row = terminal.visible_top_row()?;

    test_that::assert_that!(
        self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?),
        eq(scrolled_snapshot)
    );
    test_that::assert_that!(scrolled_top_row, lt(bottom_top_row));
    Ok(())
}

#[test]
fn test_terminal_state_visible_row_wraps_when_live_row_wraps_reports_flag() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(4, 2)?);

    let _ = terminal.process(b"abcdx");

    test_that::assert_that!(
        terminal.visible_row_wraps(),
        eq(vec![RowWrap::EndsWithSoftWrap, RowWrap::EndsBeforeSoftWrap])
    );
    Ok(())
}
