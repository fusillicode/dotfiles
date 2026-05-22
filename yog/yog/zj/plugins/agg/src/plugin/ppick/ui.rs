use std::fmt::Write;

use agg::Cmd;
use agg::GitStat;
use agg::TabIndicator;

pub const ENTRY_ROWS: usize = 3;
const PICKER_SELECTED_BG: &str = "\x1b[48;2;42;42;42m";
const TAB_DEFAULT_FG: &str = "\x1b[39m";
const PATH_STYLE: &str = "\x1b[38;2;119;119;119m";
const BRANCH_FG: &str = "\x1b[38;2;208;208;208m";
const AGENT_LABEL_FG: &str = "\x1b[38;2;218;215;255m";
const RUNNING_CMD_FG: &str = "\x1b[38;2;208;208;208m";
const SESSION_SUMMARY_FG: &str = "\x1b[38;2;189;189;189m";
const COMMIT_FG: &str = "\x1b[38;2;138;138;138m";
const RAIL_SELECTED_FG: &str = "\x1b[38;2;124;124;255m";
const SUMMARY_MAX_WIDTH: usize = 80;
const COMMIT_SUMMARY_MAX_WIDTH: usize = 80;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub struct PpickRow {
    pub selected: bool,
    pub cwd_label: String,
    pub branch_label: String,
    pub git: GitStat,
    pub cmd: Cmd,
    pub indicator: TabIndicator,
    pub session_summary: String,
}

struct LinePart<'a> {
    style: &'a str,
    value: &'a str,
}

pub fn render_frame(frame: &[PpickRow], query: &str, rows: usize, cols: usize, buf: &mut String) {
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
        let mut row_1based = 2_usize;
        for row in frame {
            for line in [
                crate::plugin::ppick::ui::cmd_line(row, cols),
                crate::plugin::ppick::ui::metadata_line(row, cols),
                crate::plugin::ui::pad("", cols),
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

#[cfg(test)]
fn path_line(row: &PpickRow, cols: usize) -> String {
    crate::plugin::ppick::ui::metadata_line(row, cols)
}

fn metadata_line(row: &PpickRow, cols: usize) -> String {
    let bg = if row.selected { PICKER_SELECTED_BG } else { "" };
    let inner_w = cols.saturating_sub(1);
    let git_parts = crate::plugin::ui::git_stat_parts(&row.git);
    let commit_label = crate::plugin::ppick::ui::commit_label(&row.git);
    let mut parts = vec![
        LinePart {
            style: PATH_STYLE,
            value: &row.cwd_label,
        },
        LinePart { style: "", value: " " },
        LinePart {
            style: BRANCH_FG,
            value: &row.branch_label,
        },
    ];
    for (color, value) in &git_parts {
        parts.push(LinePart { style: "", value: " " });
        parts.push(LinePart { style: color, value });
    }
    if !commit_label.is_empty() {
        parts.push(LinePart { style: "", value: " " });
        parts.push(LinePart {
            style: COMMIT_FG,
            value: &commit_label,
        });
    }

    let mut out = crate::plugin::ppick::ui::line_prefix(row);
    crate::plugin::ppick::ui::push_line_parts(&mut out, &parts, inner_w, bg, TAB_DEFAULT_FG);
    out.push_str(bg);
    out.push_str(TAB_DEFAULT_FG);
    crate::plugin::ui::pad_ansi(&out, cols)
}

fn cmd_line(row: &PpickRow, cols: usize) -> String {
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
    let mut out = crate::plugin::ppick::ui::line_prefix(row);
    out.push_str(&crate::plugin::ppick::ui::cmd_with_color(
        row,
        cmd_width,
        bg,
        TAB_DEFAULT_FG,
    ));
    if summary_width > 0 {
        if cmd_width > 0 {
            out.push(' ');
        }
        out.push_str(SESSION_SUMMARY_FG);
        out.push_str(&ytil_tui::display_fixed_width(&row.session_summary, summary_width));
        out.push_str(crate::plugin::ui::RESET);
    }
    out.push_str(bg);
    out.push_str(TAB_DEFAULT_FG);
    crate::plugin::ui::pad_ansi(&out, cols)
}

fn push_line_parts(out: &mut String, parts: &[LinePart<'_>], width: usize, bg: &str, fg: &str) {
    if width == 0 {
        return;
    }
    let total_width = parts.iter().map(|part| part.value.chars().count()).sum::<usize>();
    let visible_width = if total_width > width {
        width.saturating_sub(1)
    } else {
        width
    };
    let mut remaining = visible_width;
    let mut truncated_style = parts.first().map_or("", |part| part.style);
    for part in parts {
        if remaining == 0 {
            truncated_style = part.style;
            break;
        }
        let part_width = part.value.chars().count();
        let take = remaining.min(part_width);
        if take > 0 {
            crate::plugin::ppick::ui::push_line_part(out, part.style, part.value.chars().take(take), bg, fg);
        }
        remaining = remaining.saturating_sub(take);
        if take < part_width {
            truncated_style = part.style;
            break;
        }
    }
    if total_width > width {
        crate::plugin::ppick::ui::push_line_part(out, truncated_style, "…".chars(), bg, fg);
    }
}

fn push_line_part(out: &mut String, style: &str, chars: impl Iterator<Item = char>, bg: &str, fg: &str) {
    if !style.is_empty() {
        out.push_str(style);
    }
    for ch in chars {
        out.push(ch);
    }
    if !style.is_empty() {
        out.push_str(crate::plugin::ui::RESET);
        out.push_str(bg);
        out.push_str(fg);
    }
}

fn line_prefix(row: &PpickRow) -> String {
    if row.selected {
        return format!("{PICKER_SELECTED_BG}{RAIL_SELECTED_FG}▎{PICKER_SELECTED_BG}{TAB_DEFAULT_FG}");
    }
    format!(" {TAB_DEFAULT_FG}")
}

fn commit_label(git: &GitStat) -> String {
    let Some(last_commit) = git.last_commit.as_ref() else {
        return String::new();
    };
    let summary = ytil_tui::display_fixed_width(&last_commit.summary, COMMIT_SUMMARY_MAX_WIDTH);
    format!("{} {} | {summary}", last_commit.short_sha, last_commit.age)
}

fn cmd_with_color(row: &PpickRow, width: usize, bg: &str, fg: &str) -> String {
    if width == 0 {
        return String::new();
    }
    let label = crate::plugin::ui::cmd_label(&row.cmd);
    let label_style = crate::plugin::ppick::ui::cmd_label_style(&row.cmd);
    let dot_restore_fg = if label.is_empty() { fg } else { label_style };
    match crate::plugin::ui::agent_dot(row.indicator, bg, dot_restore_fg) {
        Some(dot) if width < 3 || label.is_empty() => crate::plugin::ui::pad_ansi(&dot, width),
        Some(dot) => {
            let label =
                crate::plugin::ppick::ui::styled_fixed_label(label, width.saturating_sub(2), label_style, bg, fg);
            format!("{dot} {label}")
        }
        None => crate::plugin::ppick::ui::styled_fixed_label(label, width, label_style, bg, fg),
    }
}

const fn cmd_label_style(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Agent { .. } => AGENT_LABEL_FG,
        Cmd::Running(_) => RUNNING_CMD_FG,
        Cmd::None => "",
    }
}

fn styled_fixed_label(label: &str, width: usize, style: &str, bg: &str, fg: &str) -> String {
    if width == 0 {
        return String::new();
    }
    let label = ytil_tui::display_fixed_width(label, width);
    if style.is_empty() {
        return label;
    }
    format!("{style}{label}{}{bg}{fg}", crate::plugin::ui::RESET)
}

#[cfg(test)]
mod tests {
    use agg::LastCommit;

    use super::*;

    #[test]
    fn test_render_frame_compact_entries() {
        let frame = vec![PpickRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::Running("cargo".to_string()),
            indicator: TabIndicator::NoAgent,
            session_summary: String::new(),
        }];
        let mut rendered = String::new();

        render_frame(&frame, "car", 5, 32, &mut rendered);
        let plain = plain_text(&rendered);
        let lines = [
            cmd_line(&frame[0], 32),
            path_line(&frame[0], 32),
            crate::plugin::ui::pad("", 32),
        ];

        assert2::assert!(rendered.contains("\x1b[1;1H/car"));
        assert2::assert!(rendered.contains("\x1b[2;1H"));
        assert2::assert!(plain.contains("▎cargo"));
        assert2::assert!(plain.contains("▎~/project main"));
        assert2::assert!(plain.find("▎cargo") < plain.find("▎~/project main"));
        for line in lines {
            pretty_assertions::assert_eq!(crate::plugin::ui::visible_len(&line), 32);
        }
    }

    #[test]
    fn test_render_frame_orders_agent_before_path_metadata() {
        let frame = vec![PpickRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "feature/some-very-long-branch-name".to_string(),
            git: GitStat::default(),
            cmd: Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Acknowledged),
            indicator: TabIndicator::Seen,
            session_summary: "how to solve this warning".to_string(),
        }];
        let mut rendered = String::new();

        render_frame(&frame, "", 5, 64, &mut rendered);
        let lines = [cmd_line(&frame[0], 64), path_line(&frame[0], 64)];
        let plain = plain_text(&rendered);

        assert2::assert!(plain.contains("▎cx how to solve this warning"));
        assert2::assert!(plain.contains("▎~/project feature/some-very-long-branch-name"));
        assert2::assert!(
            plain.find("▎cx how to solve this warning") < plain.find("▎~/project feature/some-very-long-branch-name")
        );
        pretty_assertions::assert_eq!(
            plain_text(&lines[1]),
            crate::plugin::ui::pad("▎~/project feature/some-very-long-branch-name", 64)
        );
        pretty_assertions::assert_eq!(
            plain_text(&lines[0]),
            crate::plugin::ui::pad("▎cx how to solve this warning", 64)
        );
    }

    #[test]
    fn test_cmd_line_busy_agent_uses_tbar_indicator_before_agent_name() {
        let row = PpickRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Busy),
            indicator: TabIndicator::Busy,
            session_summary: String::new(),
        };

        let lines = [path_line(&row, 44), cmd_line(&row, 44)];

        assert2::assert!(lines[1].contains(&format!(
            "{}{}•{}{PICKER_SELECTED_BG}{AGENT_LABEL_FG} ",
            crate::plugin::ui::BOLD,
            crate::plugin::ui::AGENT_BUSY_FG,
            crate::plugin::ui::RESET
        )));
        pretty_assertions::assert_eq!(plain_text(&lines[0]), crate::plugin::ui::pad("▎~/project main", 44));
        pretty_assertions::assert_eq!(plain_text(&lines[1]), crate::plugin::ui::pad("▎• cx", 44));
        for line in lines {
            pretty_assertions::assert_eq!(crate::plugin::ui::visible_len(&line), 44);
        }
    }

    #[test]
    fn test_path_line_trims_combined_path_metadata() {
        let row = PpickRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "feature/super-long-branch".to_string(),
            git: GitStat::default(),
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            session_summary: String::new(),
        };

        let line = path_line(&row, 20);

        pretty_assertions::assert_eq!(plain_text(&line), "▎~/project feature/…");
        pretty_assertions::assert_eq!(crate::plugin::ui::visible_len(&line), 20);
    }

    #[test]
    fn test_entry_lines_render_agent_and_muted_path_metadata() {
        let row = PpickRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat {
                last_commit: Some(LastCommit {
                    short_sha: "abc1234".to_string(),
                    age: "2m".to_string(),
                    summary: "fix branch metadata".to_string(),
                }),
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

        let lines = [path_line(&row, 80), cmd_line(&row, 80)];

        assert2::assert!(lines[0].contains(PATH_STYLE));
        assert2::assert!(!lines[0].contains(crate::plugin::ui::BOLD));
        assert2::assert!(lines[0].contains(BRANCH_FG));
        assert2::assert!(lines[0].contains(crate::plugin::ui::GIT_NEW_LINES_FG));
        assert2::assert!(lines[0].contains(crate::plugin::ui::GIT_DEL_LINES_FG));
        assert2::assert!(lines[0].contains(crate::plugin::ui::GIT_NEW_FILES_FG));
        assert2::assert!(lines[0].contains(COMMIT_FG));
        assert2::assert!(lines[1].contains(AGENT_LABEL_FG));
        assert2::assert!(lines[1].contains(SESSION_SUMMARY_FG));
        pretty_assertions::assert_eq!(
            plain_text(&lines[0]),
            crate::plugin::ui::pad("▎~/project main +2 -1 ?3 abc1234 2m | fix branch metadata", 80)
        );
        pretty_assertions::assert_eq!(plain_text(&lines[1]), crate::plugin::ui::pad("▎cx solve warning", 80));
        for line in lines {
            pretty_assertions::assert_eq!(crate::plugin::ui::visible_len(&line), 80);
        }
    }

    #[test]
    fn test_path_line_caps_commit_summary_to_80() {
        let row = PpickRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat {
                last_commit: Some(LastCommit {
                    short_sha: "abc1234".to_string(),
                    age: "1w".to_string(),
                    summary: "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz".to_string(),
                }),
                ..Default::default()
            },
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            session_summary: String::new(),
        };

        let line = path_line(&row, 120);

        pretty_assertions::assert_eq!(
            plain_text(&line),
            crate::plugin::ui::pad(
                "▎~/project main abc1234 1w | abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyza…",
                120,
            )
        );
        pretty_assertions::assert_eq!(crate::plugin::ui::visible_len(&line), 120);
    }

    #[test]
    fn test_cmd_line_trims_attached_session_summary() {
        let row = PpickRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Acknowledged),
            indicator: TabIndicator::Seen,
            session_summary: "abcdefghijklmnopqrstuvwxyz".to_string(),
        };

        let line = cmd_line(&row, 20);

        pretty_assertions::assert_eq!(plain_text(&line), "▎cx abcdefghijklmno…");
        pretty_assertions::assert_eq!(crate::plugin::ui::visible_len(&line), 20);
    }

    #[test]
    fn test_cmd_line_caps_attached_session_summary_to_80() {
        let row = PpickRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::agent(ytil_agents::agent::Agent::Codex, agg::AgentState::Acknowledged),
            indicator: TabIndicator::Seen,
            session_summary: "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz".to_string(),
        };

        let line = cmd_line(&row, 100);

        pretty_assertions::assert_eq!(
            plain_text(&line),
            crate::plugin::ui::pad(
                "▎cx abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyza…",
                100,
            )
        );
        pretty_assertions::assert_eq!(crate::plugin::ui::visible_len(&line), 100);
    }

    #[test]
    fn test_inactive_entry_lines_do_not_render_empty_rail() {
        let row = PpickRow {
            selected: false,
            cwd_label: "~/project".to_string(),
            branch_label: "main".to_string(),
            git: GitStat::default(),
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            session_summary: String::new(),
        };
        let lines = [path_line(&row, 24), cmd_line(&row, 24)];

        for line in lines {
            assert2::assert!(!plain_text(&line).contains('▏'));
            pretty_assertions::assert_eq!(crate::plugin::ui::visible_len(&line), 24);
        }
        pretty_assertions::assert_eq!(
            plain_text(&path_line(&row, 24)),
            crate::plugin::ui::pad(" ~/project main", 24)
        );
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
