use std::fmt::Write;

use agg::GitStat;
use agg::TabIndicator;

const BOLD: &str = "\x1b[1m";
const AGENT_WAITING_UNSEEN_FG: &str = "\x1b[38;2;255;0;0m";
const AGENT_BUSY_FG: &str = "\x1b[38;2;255;170;51m";
const GIT_NEW_LINES_FG: &str = "\x1b[38;2;140;228;121m";
const GIT_DEL_LINES_FG: &str = "\x1b[38;2;236;99;92m";
const GIT_NEW_FILES_FG: &str = "\x1b[38;2;0;255;255m";
const PICKER_SELECTED_BG: &str = "\x1b[48;2;50;50;50m";
const TAB_DEFAULT_FG: &str = "\x1b[39m";
const SUMMARY_FG: &str = "\x1b[38;2;119;119;119m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PickerRow {
    pub selected: bool,
    pub cwd_label: String,
    pub summary: String,
    pub label: String,
    pub marker: TabIndicator,
    pub git: GitStat,
}

pub fn render_frame(frame: &[PickerRow], query: &str, rows: usize, cols: usize, buf: &mut String) {
    if cols == 0 || rows == 0 {
        return;
    }

    let header = crate::plugin::picker::ui::pad(&format!("/{query}"), cols);
    let _ = write!(buf, "\x1b[1;1H{header}");

    let rendered_rows = if frame.is_empty() && rows > 1 {
        let empty = crate::plugin::picker::ui::pad("-", cols);
        let _ = write!(buf, "\x1b[2;1H{empty}");
        2
    } else {
        let mut last_rendered_row = 1_usize;
        for (idx, row) in frame.iter().take(rows.saturating_sub(1)).enumerate() {
            let row_1based = idx.saturating_add(2);
            let content = crate::plugin::picker::ui::row_content(row, cols);
            let _ = write!(buf, "\x1b[{row_1based};1H{content}{RESET}");
            last_rendered_row = row_1based;
        }
        last_rendered_row
    };

    for row in rendered_rows.saturating_add(1)..=rows {
        let blank = crate::plugin::picker::ui::pad("", cols);
        let _ = write!(buf, "\x1b[{row};1H{blank}");
    }
}

fn row_content(row: &PickerRow, cols: usize) -> String {
    let bg = if row.selected { PICKER_SELECTED_BG } else { "" };
    let marker = if row.selected { ">" } else { " " };
    let prefix_plain = format!("{marker} ");
    let prefix = format!("{bg}{TAB_DEFAULT_FG}{prefix_plain}");
    let prefix_w = prefix_plain.chars().count();
    if cols <= prefix_w {
        return format!(
            "{bg}{TAB_DEFAULT_FG}{}",
            crate::plugin::picker::ui::pad(&ytil_tui::display_fixed_width(&prefix_plain, cols), cols)
        );
    }

    let available = cols.saturating_sub(prefix_w);
    let git_parts = crate::plugin::picker::ui::git_stat_parts(&row.git);
    let git_wanted = git_parts
        .iter()
        .map(|(_, value)| value.chars().count())
        .sum::<usize>()
        .saturating_add(git_parts.len().saturating_sub(1));
    let mut agent_width = crate::plugin::picker::ui::agent_width(&row.label, row.marker, available);
    if agent_width > 0 && agent_width >= available {
        agent_width = available.saturating_sub(1);
    }
    let mut git_width = git_wanted;
    if git_width > 0 {
        let fixed_gap_count = usize::from(agent_width > 0).saturating_add(1);
        if git_width.saturating_add(agent_width).saturating_add(fixed_gap_count) >= available {
            git_width = 0;
        }
    }
    let git_gap = usize::from(git_width > 0);
    let agent_gap = usize::from(agent_width > 0);
    let fixed_width = git_width.saturating_add(agent_width);
    let fixed_gaps = git_gap.saturating_add(agent_gap);
    let text_width = available.saturating_sub(fixed_width).saturating_sub(fixed_gaps);
    let summary_gap = usize::from(!row.summary.is_empty() && text_width > 1);
    let min_summary_width = if row.summary.is_empty() {
        0
    } else {
        text_width.saturating_sub(1).min(24)
    };
    let path_cells = if min_summary_width == 0 {
        text_width
    } else {
        row.cwd_label
            .chars()
            .count()
            .min(text_width.saturating_sub(summary_gap).saturating_sub(min_summary_width))
    };
    let summary_cells = text_width.saturating_sub(path_cells).saturating_sub(summary_gap);
    let cwd = ytil_tui::display_fixed_width(&row.cwd_label, path_cells);
    let summary = ytil_tui::display_fixed_width(&row.summary, summary_cells);
    let agent = crate::plugin::picker::ui::agent_with_marker(&row.label, row.marker, agent_width, bg, TAB_DEFAULT_FG);
    let git = crate::plugin::picker::ui::git_stat_with_color(&git_parts, git_width, bg, TAB_DEFAULT_FG);

    let mut out = prefix;
    out.push_str(BOLD);
    out.push_str(TAB_DEFAULT_FG);
    out.push_str(&cwd);
    out.push_str(RESET);
    out.push_str(bg);
    out.push_str(TAB_DEFAULT_FG);
    if git_width > 0 {
        out.push(' ');
        out.push_str(&git);
    }
    if agent_width > 0 {
        out.push(' ');
        out.push_str(&agent);
    }
    if summary_cells > 0 {
        out.push(' ');
        out.push_str(SUMMARY_FG);
        out.push_str(&summary);
        out.push_str(RESET);
        out.push_str(bg);
        out.push_str(TAB_DEFAULT_FG);
    }
    crate::plugin::picker::ui::pad_ansi(&out, cols)
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
    out.push_str(RESET);
    out.push_str(bg);
    out.push_str(fg);
    out
}

fn git_stat_parts(git_stat: &GitStat) -> Vec<(&'static str, String)> {
    let mut stats = Vec::new();
    if git_stat.insertions > 0 {
        stats.push((GIT_NEW_LINES_FG, format!("+{}", git_stat.insertions)));
    }
    if git_stat.deletions > 0 {
        stats.push((GIT_DEL_LINES_FG, format!("-{}", git_stat.deletions)));
    }
    if git_stat.new_files > 0 {
        stats.push((GIT_NEW_FILES_FG, format!("?{}", git_stat.new_files)));
    }
    stats
}

fn agent_width(label: &str, marker: TabIndicator, available: usize) -> usize {
    let min_dot_width = usize::from(matches!(marker, TabIndicator::Unseen | TabIndicator::Busy));
    if available <= min_dot_width {
        return min_dot_width.min(available);
    }
    let label_width = label.chars().count();
    let wanted = match marker {
        TabIndicator::Unseen | TabIndicator::Busy if label_width > 0 => label_width.saturating_add(2),
        TabIndicator::Unseen | TabIndicator::Busy => 1,
        TabIndicator::NoAgent | TabIndicator::Seen => label_width,
    };
    wanted.min(available.min(12))
}

fn agent_with_marker(label: &str, marker: TabIndicator, width: usize, bg: &str, fg: &str) -> String {
    if width == 0 {
        return String::new();
    }
    let dot = match marker {
        TabIndicator::Unseen => Some(format!("{BOLD}{AGENT_WAITING_UNSEEN_FG}•{RESET}{bg}{fg}")),
        TabIndicator::Busy => Some(format!("{BOLD}{AGENT_BUSY_FG}•{RESET}{bg}{fg}")),
        TabIndicator::NoAgent | TabIndicator::Seen => None,
    };
    match dot {
        Some(dot) if label.is_empty() || width < 3 => dot,
        Some(dot) => {
            let label = ytil_tui::display_fixed_width(label, width.saturating_sub(2));
            format!("{dot} {label}")
        }
        None => ytil_tui::display_fixed_width(label, width),
    }
}

fn visible_len(s: &str) -> usize {
    let mut len = 0_usize;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            len = len.saturating_add(1);
        }
    }
    len
}

fn pad(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= width {
        return chars.into_iter().take(width).collect();
    }
    let mut out = String::with_capacity(width);
    for c in &chars {
        out.push(*c);
    }
    for _ in chars.len()..width {
        out.push(' ');
    }
    out
}

fn pad_ansi(s: &str, width: usize) -> String {
    let len = crate::plugin::picker::ui::visible_len(s);
    if len >= width {
        return s.to_string();
    }
    let mut out = String::from(s);
    for _ in len..width {
        out.push(' ');
    }
    out
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_render_frame_compact_rows() {
        let frame = vec![PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            summary: String::new(),
            label: "cargo".to_string(),
            marker: TabIndicator::NoAgent,
            git: GitStat::default(),
        }];
        let mut rendered = String::new();

        render_frame(&frame, "car", 3, 24, &mut rendered);
        let plain = plain_text(&rendered);

        assert2::assert!(rendered.contains("\x1b[1;1H/car"));
        assert2::assert!(rendered.contains("\x1b[2;1H"));
        assert2::assert!(plain.contains("> ~/project cargo"));
        assert_eq!(visible_len(&row_content(&frame[0], 24)), 24);
    }

    #[test]
    fn test_render_frame_orders_path_agent_and_session_summary() {
        let frame = vec![PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            summary: "how to solve this warning".to_string(),
            label: "cx".to_string(),
            marker: TabIndicator::Seen,
            git: GitStat::default(),
        }];
        let mut rendered = String::new();

        render_frame(&frame, "", 3, 52, &mut rendered);
        let content = row_content(&frame[0], 52);

        assert2::assert!(plain_text(&rendered).contains("> ~/project cx how to solve this warning"));
        assert_eq!(
            plain_text(&content),
            pad("> ~/project cx how to solve this warning", 52)
        );
        assert_eq!(visible_len(&content), 52);
    }

    #[test]
    fn test_row_content_busy_agent_uses_tab_bar_indicator_before_agent_name() {
        let row = PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            summary: "solve warning".to_string(),
            label: "cx".to_string(),
            marker: TabIndicator::Busy,
            git: GitStat::default(),
        };

        let content = row_content(&row, 44);

        assert2::assert!(content.contains(&format!("{BOLD}{AGENT_BUSY_FG}•{RESET}")));
        assert_eq!(plain_text(&content), pad("> ~/project • cx solve warning", 44));
        assert_eq!(visible_len(&content), 44);
    }

    #[test]
    fn test_row_content_orders_path_git_agent_and_session_summary() {
        let row = PickerRow {
            selected: true,
            cwd_label: "~/project".to_string(),
            summary: "solve warning".to_string(),
            label: "cx".to_string(),
            marker: TabIndicator::Seen,
            git: GitStat {
                insertions: 2,
                deletions: 1,
                new_files: 3,
                is_worktree: false,
            },
        };

        let content = row_content(&row, 52);

        assert2::assert!(content.contains(GIT_NEW_LINES_FG));
        assert2::assert!(content.contains(GIT_DEL_LINES_FG));
        assert2::assert!(content.contains(GIT_NEW_FILES_FG));
        assert_eq!(plain_text(&content), pad("> ~/project +2 -1 ?3 cx solve warning", 52));
        assert_eq!(visible_len(&content), 52);
    }

    fn plain_text(value: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in value.chars() {
            if in_escape {
                if c == 'm' {
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
