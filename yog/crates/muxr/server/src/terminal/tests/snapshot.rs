use super::*;

#[test]
fn test_terminal_state_candidate_line_spans_when_wraps_mix_skips_overlapping_suffixes() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(10, 4)?);
    let _output = terminal.process(b"abcdefghijk\r\nlast");
    let tail = terminal.live_tail_text(12);
    let spans: Vec<Vec<_>> = tail
        .candidate_line_spans()
        .map(|span| span.map(str::trim_end).collect())
        .collect();
    test_that::assert_that!(
        spans,
        eq(vec![
            vec!["abcdefghijk", "last", ""],
            vec!["k", "last", ""],
            vec!["last", ""],
            vec![""]
        ])
    );
    Ok(())
}

#[test]
fn test_terminal_state_candidate_line_spans_when_rows_are_short_retains_right_edge_padding() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(4, 3)?);
    let _output = terminal.process(b"full\r\nend");
    let tail = terminal.live_tail_text(12);
    let spans: Vec<Vec<_>> = tail.candidate_line_spans().map(Iterator::collect).collect();
    test_that::assert_that!(
        spans,
        eq(vec![vec!["full", "end ", "    "], vec!["end ", "    "], vec!["    "]])
    );
    Ok(())
}

#[test]
fn test_terminal_state_live_tail_when_scrolled_reads_live_rows_without_changing_viewport() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(80, 3)?);
    let _output = terminal.process("old\r\none\r\ntwo\r\nWorking (6m 35s • ctrl+x to interrupt)".as_bytes());
    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));
    let before = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    test_that::assert_that!(
        terminal.live_tail_text(2).candidate_lines().collect::<Vec<_>>(),
        eq(vec!["two", "Working (6m 35s • ctrl+x to interrupt)"])
    );
    test_that::assert_that!(terminal.render_snapshot(TerminalSnapshotScope::Full)?, eq(before));
    Ok(())
}

#[test]
fn test_terminal_state_live_tail_when_wrapped_joins_status_and_preserves_damage() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(20, 2)?);
    let _baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let _output = terminal.process("\x1b[32mWorking (6m 35s • ctrl+x to interrupt)\x1b[0m".as_bytes());
    test_that::assert_that!(
        terminal.live_tail_text(12).candidate_lines().collect::<Vec<_>>(),
        eq(vec!["Working (6m 35s • ctrl+x to interrupt)", "rl+x to interrupt)"])
    );
    test_that::assert_that!(
        terminal
            .render_snapshot(TerminalSnapshotScope::ChangedRows)?
            .rows()
            .len(),
        eq(2)
    );
    Ok(())
}

#[test]
fn test_terminal_state_live_tail_when_window_is_bounded_excludes_older_rows() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(80, 13)?);
    let _output = terminal.process("Working (1s • esc to interrupt)\r\n".as_bytes());
    test_that::assert_that!(
        terminal.live_tail_text(12).candidate_lines().collect::<Vec<_>>(),
        eq(vec![""; 12])
    );
    test_that::assert_that!(
        terminal.live_tail_text(0).candidate_lines().collect::<Vec<_>>(),
        eq(Vec::<&str>::new())
    );
    Ok(())
}

#[test]
fn test_terminal_state_live_tail_when_alternate_screen_is_active_reads_active_grid() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(80, 1)?);
    let _output = terminal.process("Worked for 1s • 11:37\x1b[?1049h\x1b[HWorking (1s • esc to interrupt)".as_bytes());
    test_that::assert_that!(
        terminal.live_tail_text(12).candidate_lines().collect::<Vec<_>>(),
        eq(vec!["Working (1s • esc to interrupt)"])
    );
    Ok(())
}

#[test]
fn test_terminal_state_when_completion_is_repainted_reports_damage_without_screen_changes() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(80, 3)?);
    let repaint = "\x1b[2;1H  Worked for 16m 15s • 14:18";
    let _initial_output = terminal.process(repaint.as_bytes());
    let before = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

    let output = terminal.process(repaint.as_bytes());

    test_that::assert_that!(
        output,
        eq(TerminalProcessOutcome::ScreenDirty {
            replies: TerminalReplies::default()
        })
    );
    test_that::assert_that!(terminal.render_snapshot(TerminalSnapshotScope::Full)?, eq(before));
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_output_processed_contains_screen() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let outcome = terminal.process(b"hi");
    self::assert_replies_eq(&outcome.into_replies(), &[]);
    let snapshot = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let Some(row) = snapshot.rows().first() else {
        return Err(report!("expected first render row"));
    };
    let rendered = row.cells().iter().take(2).map(RenderCell::text).collect::<String>();

    test_that::assert_that!(rendered, eq("hi"));
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_osc8_span_is_rendered_shares_uri_allocation() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 1)?);
    let _outcome = terminal.process(b"\x1b]8;;https://example.com\x07click\x1b]8;;\x07");
    let snapshot = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let Some(row) = snapshot.rows().first() else {
        return Err(report!("expected first render row"));
    };
    let mut hyperlinks = row.cells().iter().filter_map(RenderCell::hyperlink);
    let Some(first) = hyperlinks.next() else {
        return Err(report!("expected first rendered hyperlink"));
    };
    let Some(second) = hyperlinks.next() else {
        return Err(report!("expected second rendered hyperlink"));
    };

    test_that::assert_that!(std::ptr::eq(first.uri(), second.uri()), eq(true));
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_output_contains_horizontal_tab_expands_it_to_spaces() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 1)?);

    let _outcome = terminal.process(b"\tmodified");
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, starts_with("        modified"));
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_orphan_combining_mark_precedes_horizontal_tab_keeps_alacritty_cell_width()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(16, 1)?);

    let _outcome = terminal.process("\u{301}\tmodified".as_bytes());
    let rendered = self::snapshot_text(&terminal.render_snapshot(TerminalSnapshotScope::Full)?);

    test_that::assert_that!(rendered, not(contains_substring("\t")));
    test_that::assert_that!(rendered, starts_with(" \u{301}       modified"));
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_zwj_emoji_uses_alacritty_cell_width_keeps_following_text_at_column_four()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 1)?);

    let _outcome = terminal.process("\x1b[?2027h🥨‍🍙X".as_bytes());
    let snapshot = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let Some(row) = snapshot.rows().first() else {
        return Err(report!("expected first render row"));
    };
    let Some(first_emoji) = row.cells().first() else {
        return Err(report!("expected first emoji cell"));
    };
    let Some(first_continuation) = row.cells().get(1) else {
        return Err(report!("expected first emoji continuation cell"));
    };
    let Some(second_emoji) = row.cells().get(2) else {
        return Err(report!("expected second emoji cell"));
    };
    let Some(second_continuation) = row.cells().get(3) else {
        return Err(report!("expected second emoji continuation cell"));
    };
    let Some(following) = row.cells().get(4) else {
        return Err(report!("expected following text cell"));
    };

    test_that::assert_that!(first_emoji.text(), eq("🥨‍"));
    test_that::assert_that!(first_emoji.width(), eq(RenderCellWidth::Wide));
    test_that::assert_that!(first_continuation.width(), eq(RenderCellWidth::WideContinuation));
    test_that::assert_that!(second_emoji.text(), eq("🍙"));
    test_that::assert_that!(second_emoji.width(), eq(RenderCellWidth::Wide));
    test_that::assert_that!(second_continuation.width(), eq(RenderCellWidth::WideContinuation));
    test_that::assert_that!(following.text(), eq("X"));
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_variation_selector_uses_alacritty_cell_width_keeps_following_text_at_column_one()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 1)?);

    let _outcome = terminal.process("❤️X".as_bytes());
    let snapshot = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let Some(row) = snapshot.rows().first() else {
        return Err(report!("expected first render row"));
    };
    let Some(emoji) = row.cells().first() else {
        return Err(report!("expected variation-selector emoji cell"));
    };
    let Some(following) = row.cells().get(1) else {
        return Err(report!("expected following text cell"));
    };

    test_that::assert_that!(emoji.text(), eq("❤️"));
    test_that::assert_that!(emoji.width(), eq(RenderCellWidth::Narrow));
    test_that::assert_that!(following.text(), eq("X"));
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_cursor_shape_is_set_returns_shape() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b[6 q").into_replies(), &[]);

    test_that::assert_that!(
        terminal.render_snapshot(TerminalSnapshotScope::Full)?.cursor().shape,
        eq(RenderCursorShape::SteadyBar)
    );
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_steady_block_is_explicit_returns_explicit_shape() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _outcome = terminal.process(b"\x1b[2 q");

    test_that::assert_that!(
        terminal.render_snapshot(TerminalSnapshotScope::Full)?.cursor().shape,
        eq(RenderCursorShape::SteadyBlock)
    );
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_osc_50_sets_initial_block_returns_explicit_shape() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let outcome = terminal.process(b"\x1b]50;CursorShape=0\x07");

    test_that::assert_that!(outcome.screen_dmg(), eq(TerminalScreenDmg::Dirty));
    test_that::assert_that!(
        terminal.render_snapshot(TerminalSnapshotScope::Full)?.cursor().shape,
        eq(RenderCursorShape::SteadyBlock)
    );
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_osc_50_changes_beam_to_block_returns_explicit_block() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);
    let _beam = terminal.process(b"\x1b]50;CursorShape=1\x07");
    test_that::assert_that!(
        terminal.render_snapshot(TerminalSnapshotScope::Full)?.cursor().shape,
        eq(RenderCursorShape::SteadyBar)
    );

    let block = terminal.process(b"\x1b]50;CursorShape=0\x07");

    test_that::assert_that!(block.screen_dmg(), eq(TerminalScreenDmg::Dirty));
    test_that::assert_that!(
        terminal.render_snapshot(TerminalSnapshotScope::Full)?.cursor().shape,
        eq(RenderCursorShape::SteadyBlock)
    );
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_osc_50_sequence_is_split_returns_explicit_shape() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let first = terminal.process(b"\x1b]50;Cursor");
    let second = terminal.process(b"Shape=0\x1b\\");

    test_that::assert_that!(first.screen_dmg(), eq(TerminalScreenDmg::Clean));
    test_that::assert_that!(second.screen_dmg(), eq(TerminalScreenDmg::Dirty));
    test_that::assert_that!(
        terminal.render_snapshot(TerminalSnapshotScope::Full)?.cursor().shape,
        eq(RenderCursorShape::SteadyBlock)
    );
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_cursor_shape_sequence_is_split_returns_shape() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    let _first = terminal.process(b"\x1b[2 ");
    let second = terminal.process(b"q");

    test_that::assert_that!(second.screen_dmg(), eq(TerminalScreenDmg::Dirty));
    test_that::assert_that!(
        terminal.render_snapshot(TerminalSnapshotScope::Full)?.cursor().shape,
        eq(RenderCursorShape::SteadyBlock)
    );
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_terminal_resets_clears_cursor_shape() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);

    self::assert_replies_eq(&terminal.process(b"\x1b[6 q\x1bc").into_replies(), &[]);

    test_that::assert_that!(
        terminal.render_snapshot(TerminalSnapshotScope::Full)?.cursor().shape,
        eq(RenderCursorShape::Default)
    );
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_viewport_is_scrolled_hides_live_cursor() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
    let _output = terminal.process(b"one\r\ntwo\r\nthree");

    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    test_that::assert_that!(
        terminal
            .render_snapshot(TerminalSnapshotScope::Full)?
            .cursor()
            .visibility,
        eq(muxr_core::RenderCursorVisibility::Hidden)
    );

    test_that::assert_that!(terminal.scroll_to_bottom(), eq(TerminalScrollMove::Moved));
    test_that::assert_that!(
        terminal
            .render_snapshot(TerminalSnapshotScope::Full)?
            .cursor()
            .visibility,
        eq(muxr_core::RenderCursorVisibility::Visible)
    );
    Ok(())
}

#[test]
fn test_terminal_state_snapshot_when_scrolled_after_narrowing_resize_fits_history_to_current_width()
-> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&TerminalSize::new(8, 2)?);
    let _ = terminal.process(b"one\r\ntwo\r\nthree");
    test_that::assert_that!(terminal.scroll(PaneScrollDirection::Up), eq(TerminalScrollMove::Moved));

    let before = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let before_widths = before
        .rows()
        .iter()
        .map(RenderRowSpan::width)
        .collect::<rootcause::Result<Vec<_>>>()?;
    test_that::assert_that!(before_widths, eq(vec![8, 8]));

    terminal.resize(&TerminalSize::new(4, 2)?);
    let after = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let after_widths = after
        .rows()
        .iter()
        .map(RenderRowSpan::width)
        .collect::<rootcause::Result<Vec<_>>>()?;
    test_that::assert_that!(after_widths, eq(vec![4, 4]));
    Ok(())
}

#[test]
fn test_terminal_state_render_snapshot_when_one_row_changes_returns_only_that_row() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);
    let baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    test_that::assert_that!(baseline.rows().len(), eq(usize::from(terminal_size()?.rows())));

    let _outcome = terminal.process(b"\x1b[2;1Hchanged");
    let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

    test_that::assert_that!(
        update.rows().iter().map(RenderRowSpan::row).collect::<Vec<_>>(),
        eq(vec![1])
    );
    Ok(())
}

#[test]
fn test_terminal_state_render_snapshot_when_live_partial_region_scrolls_returns_only_region_rows()
-> rootcause::Result<()> {
    let size = TerminalSize::new(8, 4)?;
    let mut terminal = self::terminal_state(&size);
    let _setup = terminal.process(b"\x1b[1;1Hone\x1b[2;1Htwo\x1b[3;1Hfixed-a\x1b[4;1Hfixed-b\x1b[1;2r");
    let mut cached = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

    let _scroll = terminal.process(b"\x1b[1S");
    let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

    test_that::assert_that!(
        update.rows().iter().map(RenderRowSpan::row).collect::<Vec<_>>(),
        eq(vec![0, 1])
    );
    let _changed_rows = cached.apply_update(update)?;
    test_that::assert_that!(cached, eq(terminal.render_snapshot(TerminalSnapshotScope::Full)?));
    Ok(())
}

#[test]
fn test_terminal_state_render_snapshot_when_only_cursor_moves_returns_no_rows() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);
    let _baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

    let _outcome = terminal.process(b"\x1b[2;2H");
    let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

    test_that::assert_that!(update.rows().len(), eq(0));
    test_that::assert_that!(update.cursor().row, eq(1));
    test_that::assert_that!(update.cursor().col, eq(1));
    Ok(())
}

#[test]
fn test_terminal_state_render_snapshot_when_viewport_moves_returns_all_rows() -> rootcause::Result<()> {
    let size = terminal_size()?;
    let mut terminal = self::terminal_state(&size);
    let _outcome = terminal.process(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
    let _baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

    test_that::assert_that!(update.rows().len(), eq(usize::from(size.rows())));
    Ok(())
}

#[test]
fn test_terminal_state_render_snapshot_when_scrolled_history_is_evicted_returns_all_rows() -> rootcause::Result<()> {
    let size = TerminalSize::new(8, 3)?;
    let mut scrollback = MuxrConfig::new()?.scrollback;
    scrollback.rows = 2;
    let mut terminal = TerminalState::with_scrollback(&size, scrollback);
    let _initial = terminal.process(b"\x1b[1;1HA\x1b[2;1HB\x1b[3;1Hfixed\x1b[1;2r\x1b[1S");
    let _second = terminal.process(b"\x1b[2;1HC\x1b[1S");
    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    test_that::assert_that!(
        terminal.scroll_one_line(PaneScrollDirection::Up),
        eq(TerminalScrollMove::Moved)
    );
    let mut cached = terminal.render_snapshot(TerminalSnapshotScope::Full)?;

    let _eviction = terminal.process(b"\x1b[2;1HD\x1b[1S");
    let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;
    test_that::assert_that!(update.rows().len(), eq(usize::from(size.rows())));
    let _changed_rows = cached.apply_update(update)?;

    test_that::assert_that!(cached, eq(terminal.render_snapshot(TerminalSnapshotScope::Full)?));
    Ok(())
}

#[test]
fn test_terminal_state_render_snapshot_when_resized_returns_all_rows() -> rootcause::Result<()> {
    let mut terminal = self::terminal_state(&terminal_size()?);
    let _baseline = terminal.render_snapshot(TerminalSnapshotScope::Full)?;
    let resized = TerminalSize::new(10, 5)?;

    terminal.resize(&resized);
    let update = terminal.render_snapshot(TerminalSnapshotScope::ChangedRows)?;

    test_that::assert_that!(update.rows().len(), eq(usize::from(resized.rows())));
    test_that::assert_that!(update.size(), eq(&resized));
    Ok(())
}
