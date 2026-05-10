use std::fmt::Write;

use agg::Cmd;
use agg::GitStat;
use agg::TabIndicator;

pub const ENTRY_ROWS: usize = 3;
const PICKER_SELECTED_BG: &str = "\x1b[48;2;50;50;50m";
const TAB_DEFAULT_FG: &str = "\x1b[39m";
const SUMMARY_FG: &str = "\x1b[38;2;119;119;119m";
const RAIL_SELECTED_FG: &str = "\x1b[38;2;106;106;223m";
const SUMMARY_MAX_WIDTH: usize = 42;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PickerRow {
    pub selected: bool,
    pub cwd_label: String,
    pub branch_label: String,
    pub git: GitStat,
    pub cmd: Cmd,
    pub indicator: TabIndicator,
    pub session_summary: String,
}

pub fn render_frame(frame: &[PickerRow], query: &str, rows: usize, cols: usize, buf: &mut String) {
    if cols == 0 || rows == 0 {
        return;
    }

    let header = crate::plugin::ui::pad(&format!("/{query}"), cols);
    let _ = write!(buf, "\x1b[1;1H{header}");

    let rendered_rows = if frame.is_empty() && rows > 1 {
        let empty = crate::plugin::ui::pad("-", cols);
        let _ = write!(buf, "\x1b[2;1H{empty}");
        2
    } else {
        let capacity = rows.saturating_sub(1) / ENTRY_ROWS;
        let selected = frame.iter().position(|row| row.selected).unwrap_or(0);
        let start = selected.saturating_add(1).saturating_sub(capacity);
        let mut row_1based = 2_usize;
        for row in frame.iter().skip(start).take(capacity) {
            for line in [
                crate::plugin::picker::ui::path_line(row, cols),
                crate::plugin::picker::ui::info_line(row, cols),
                crate::plugin::picker::ui::cmd_line(row, cols),
            ] {
                let _ = write!(buf, "\x1b[{row_1based};1H{line}{}", crate::plugin::ui::RESET);
                row_1based = row_1based.saturating_add(1);
            }
        }
        row_1based.saturating_sub(1)
    };

    for row in rendered_rows.saturating_add(1)..=rows {
        let blank = crate::plugin::ui::pad("", cols);
        let _ = write!(buf, "\x1b[{row};1H{blank}");
    }
}

fn path_line(row: &PickerRow, cols: usize) -> String {
    let bg = if row.selected { PICKER_SELECTED_BG } else { "" };
    let inner_w = cols.saturating_sub(1);
    let mut out = crate::plugin::picker::ui::line_prefix(row);
    if row.selected {
        out.push_str(crate::plugin::ui::BOLD);
    }
    out.push_str(TAB_DEFAULT_FG);
    out.push_str(&ytil_tui::display_fixed_width(&row.cwd_label, inner_w));
    out.push_str(crate::plugin::ui::RESET);
    out.push_str(bg);
    out.push_str(TAB_DEFAULT_FG);
    crate::plugin::ui::pad_ansi(&out, cols)
}

fn info_line(row: &PickerRow, cols: usize) -> String {
    let bg = if row.selected { PICKER_SELECTED_BG } else { "" };
    let inner_w = cols.saturating_sub(1);
    let available = inner_w;
    let git_parts = crate::plugin::ui::git_stat_parts(&row.git);
    let git_wanted = git_parts
        .iter()
        .map(|(_, value)| value.chars().count())
        .sum::<usize>()
        .saturating_add(git_parts.len().saturating_sub(1));
    let mut branch_width = row.branch_label.chars().count();
    let mut git_width = git_wanted;
    let mut used_width = branch_width
        .saturating_add(git_width)
        .saturating_add(usize::from(branch_width > 0 && git_width > 0));
    if used_width > available {
        git_width = 0;
    }
    used_width = branch_width
        .saturating_add(git_width)
        .saturating_add(usize::from(branch_width > 0 && git_width > 0));
    if used_width > available {
        branch_width = branch_width.min(available);
    }
    let branch = ytil_tui::display_fixed_width(&row.branch_label, branch_width);
    let git = crate::plugin::picker::ui::git_stat_with_color(&git_parts, git_width, bg, TAB_DEFAULT_FG);

    let mut out = crate::plugin::picker::ui::line_prefix(row);
    if branch_width > 0 {
        out.push_str(&branch);
    }
    if git_width > 0 {
        if branch_width > 0 {
            out.push(' ');
        }
        out.push_str(&git);
    }
    out.push_str(bg);
    out.push_str(TAB_DEFAULT_FG);
    crate::plugin::ui::pad_ansi(&out, cols)
}

fn cmd_line(row: &PickerRow, cols: usize) -> String {
    let bg = if row.selected { PICKER_SELECTED_BG } else { "" };
    let inner_w = cols.saturating_sub(1);
    let cmd = crate::plugin::ui::display_left(row.indicator, &row.cmd, bg, TAB_DEFAULT_FG);
    let cmd_wanted = crate::plugin::ui::visible_len(&cmd).min(inner_w);
    let summary_wanted = row.session_summary.chars().count().min(SUMMARY_MAX_WIDTH);
    let summary_width = if summary_wanted > 0 && cmd_wanted.saturating_add(1) < inner_w {
        summary_wanted.min(inner_w.saturating_sub(cmd_wanted).saturating_sub(1))
    } else {
        0
    };
    let cmd_width = if summary_width > 0 { cmd_wanted } else { inner_w };
    let mut out = crate::plugin::picker::ui::line_prefix(row);
    out.push_str(&crate::plugin::picker::ui::cmd_with_color(
        row,
        cmd_width,
        bg,
        TAB_DEFAULT_FG,
    ));
    if summary_width > 0 {
        if cmd_width > 0 {
            out.push(' ');
        }
        out.push_str(SUMMARY_FG);
        out.push_str(&ytil_tui::display_fixed_width(&row.session_summary, summary_width));
        out.push_str(crate::plugin::ui::RESET);
    }
    out.push_str(bg);
    out.push_str(TAB_DEFAULT_FG);
    crate::plugin::ui::pad_ansi(&out, cols)
}

fn line_prefix(row: &PickerRow) -> String {
    if row.selected {
        return format!("{PICKER_SELECTED_BG}{RAIL_SELECTED_FG}▎{PICKER_SELECTED_BG}{TAB_DEFAULT_FG}");
    }
    format!(" {TAB_DEFAULT_FG}")
}

fn git_stat_with_color(parts: &[(&'static str, String)], width: usize, bg: &str, fg: &str) -> String {
    if width == 0 {
        return String::new();
    }
    if parts.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (idx, (color, value)) in parts.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        out.push_str(color);
        out.push_str(value);
    }
    out.push_str(crate::plugin::ui::RESET);
    out.push_str(bg);
    out.push_str(fg);
    out
}

fn cmd_with_color(row: &PickerRow, width: usize, bg: &str, fg: &str) -> String {
    if width == 0 {
        return String::new();
    }
    let rendered = crate::plugin::ui::display_left(row.indicator, &row.cmd, bg, fg);
    if crate::plugin::ui::visible_len(&rendered) <= width {
        return crate::plugin::ui::pad_ansi(&rendered, width);
    }
    let label = crate::plugin::ui::cmd_label(&row.cmd);
    match crate::plugin::ui::agent_dot(row.indicator, bg, fg) {
        Some(dot) if width < 3 || label.is_empty() => crate::plugin::ui::pad_ansi(&dot, width),
        Some(dot) => {
            let label = ytil_tui::display_fixed_width(label, width.saturating_sub(2));
            format!("{dot} {label}")
        }
        None => ytil_tui::display_fixed_width(label, width),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_render_frame_compact_entries() {
        let frame = vec![PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::Running("cargo".to_string()),
            indicator: TabIndicator::NoAgent,
            session_summary: String::new(),
        }];
        let mut rendered = String::new();

        render_frame(&frame, "car", 5, 24, &mut rendered);
        let plain = plain_text(&rendered);
        let lines = [
            path_line(&frame[0], 24),
            info_line(&frame[0], 24),
            cmd_line(&frame[0], 24),
        ];

        assert2::assert!(rendered.contains("\x1b[1;1H/car"));
        assert2::assert!(rendered.contains("\x1b[2;1H"));
        assert2::assert!(plain.contains("▎~/project"));
        assert2::assert!(plain.contains("▎main"));
        assert2::assert!(plain.contains("▎cargo"));
        for line in lines {
            assert_eq!(crate::plugin::ui::visible_len(&line), 24);
        }
    }

    #[test]
    fn test_render_frame_orders_path_branch_and_agent() {
        let frame = vec![PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "feature/some-very-long-branch-name".to_string(),
            git: GitStat::default(),
            cmd: Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Acknowledged),
            indicator: TabIndicator::Seen,
            session_summary: "how to solve this warning".to_string(),
        }];
        let mut rendered = String::new();

        render_frame(&frame, "", 5, 52, &mut rendered);
        let lines = [
            path_line(&frame[0], 52),
            info_line(&frame[0], 52),
            cmd_line(&frame[0], 52),
        ];

        assert2::assert!(plain_text(&rendered).contains("▎~/project"));
        assert2::assert!(plain_text(&rendered).contains("▎feature/some-very-long-branch-name"));
        assert2::assert!(plain_text(&rendered).contains("▎cx how to solve this warning"));
        assert_eq!(plain_text(&lines[0]), crate::plugin::ui::pad("▎~/project", 52));
        assert_eq!(
            plain_text(&lines[1]),
            crate::plugin::ui::pad("▎feature/some-very-long-branch-name", 52)
        );
        assert_eq!(
            plain_text(&lines[2]),
            crate::plugin::ui::pad("▎cx how to solve this warning", 52)
        );
    }

    #[test]
    fn test_cmd_line_busy_agent_uses_tab_bar_indicator_before_agent_name() {
        let row = PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Busy),
            indicator: TabIndicator::Busy,
            session_summary: String::new(),
        };

        let lines = [path_line(&row, 44), info_line(&row, 44), cmd_line(&row, 44)];

        assert2::assert!(lines[2].contains(&format!(
            "{}{}•{}{PICKER_SELECTED_BG}{TAB_DEFAULT_FG} ",
            crate::plugin::ui::BOLD,
            crate::plugin::ui::AGENT_BUSY_FG,
            crate::plugin::ui::RESET
        )));
        assert_eq!(plain_text(&lines[1]), crate::plugin::ui::pad("▎main", 44));
        assert_eq!(plain_text(&lines[2]), crate::plugin::ui::pad("▎• cx", 44));
        for line in lines {
            assert_eq!(crate::plugin::ui::visible_len(&line), 44);
        }
    }

    #[test]
    fn test_info_line_clips_long_branch_instead_of_hiding_it() {
        let row = PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "feature/super-long-branch".to_string(),
            git: GitStat::default(),
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            session_summary: String::new(),
        };

        let line = info_line(&row, 12);

        assert_eq!(plain_text(&line), "▎feature/su…");
        assert_eq!(crate::plugin::ui::visible_len(&line), 12);
    }

    #[test]
    fn test_entry_lines_order_path_git_and_agent() {
        let row = PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat {
                insertions: 2,
                deletions: 1,
                new_files: 3,
                is_worktree: false,
                ..Default::default()
            },
            cmd: Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Acknowledged),
            indicator: TabIndicator::Seen,
            session_summary: "solve warning".to_string(),
        };

        let lines = [path_line(&row, 52), info_line(&row, 52), cmd_line(&row, 52)];

        assert2::assert!(lines[1].contains(crate::plugin::ui::GIT_NEW_LINES_FG));
        assert2::assert!(lines[1].contains(crate::plugin::ui::GIT_DEL_LINES_FG));
        assert2::assert!(lines[1].contains(crate::plugin::ui::GIT_NEW_FILES_FG));
        assert_eq!(plain_text(&lines[1]), crate::plugin::ui::pad("▎main +2 -1 ?3", 52));
        assert_eq!(plain_text(&lines[2]), crate::plugin::ui::pad("▎cx solve warning", 52));
        for line in lines {
            assert_eq!(crate::plugin::ui::visible_len(&line), 52);
        }
    }

    #[test]
    fn test_cmd_line_trims_attached_session_summary() {
        let row = PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Acknowledged),
            indicator: TabIndicator::Seen,
            session_summary: "abcdefghijklmnopqrstuvwxyz".to_string(),
        };

        let line = cmd_line(&row, 20);

        assert_eq!(plain_text(&line), "▎cx abcdefghijklmno…");
        assert_eq!(crate::plugin::ui::visible_len(&line), 20);
    }

    #[test]
    fn test_inactive_entry_lines_do_not_render_empty_rail() {
        let row = PickerRow {
            selected: false,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            session_summary: String::new(),
        };
        let lines = [path_line(&row, 24), info_line(&row, 24), cmd_line(&row, 24)];

        for line in lines {
            assert2::assert!(!plain_text(&line).contains('▏'));
            assert_eq!(crate::plugin::ui::visible_len(&line), 24);
        }
        assert_eq!(
            plain_text(&path_line(&row, 24)),
            crate::plugin::ui::pad(" ~/project", 24)
        );
    }

    #[test]
    fn test_render_frame_keeps_selected_entry_visible() {
        let frame = vec![
            PickerRow {
                selected: false,
                cwd_label: "~/first".to_string(),
                branch_label: "main".to_string(),
                git: GitStat::default(),
                cmd: Cmd::None,
                indicator: TabIndicator::NoAgent,
                session_summary: String::new(),
            },
            PickerRow {
                selected: false,
                cwd_label: "~/second".to_string(),
                branch_label: "main".to_string(),
                git: GitStat::default(),
                cmd: Cmd::None,
                indicator: TabIndicator::NoAgent,
                session_summary: String::new(),
            },
            PickerRow {
                selected: true,
                cwd_label: "~/third".to_string(),
                branch_label: "main".to_string(),
                git: GitStat::default(),
                cmd: Cmd::None,
                indicator: TabIndicator::NoAgent,
                session_summary: String::new(),
            },
        ];
        let mut rendered = String::new();

        render_frame(&frame, "", 7, 24, &mut rendered);
        let plain = plain_text(&rendered);

        assert2::assert!(!plain.contains("~/first"));
        assert2::assert!(plain.contains("~/second"));
        assert2::assert!(plain.contains("~/third"));
    }

    fn plain_text(value: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in value.chars() {
            if in_escape {
                if c.is_ascii_alphabetic() {
                    in_escape = false;
                }
            } else if c == '\x1b' {
                in_escape = true;
            } else {
                out.push(c);
            }
        }
        out
    }
}
