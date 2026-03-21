use std::fmt::Write;
use std::path::Path;

use agm_core::GitStat;
use zellij_tile::prelude::*;

use super::PaneData;

const INFO_ROWS: usize = 1;
const MARKER: &str = " ";
const SEPARATOR: char = '\u{2502}';

const SEP_COLOR: &str = "\x1b[38;2;34;34;34m";
const GREEN: &str = "\x1b[38;2;0;255;0m";
const RED: &str = "\x1b[38;2;255;0;0m";
const CYAN: &str = "\x1b[38;2;0;255;255m";
const DIM: &str = "\x1b[38;2;100;100;110m";
const AMBER: &str = "\x1b[38;2;255;255;0m";
const ACTIVE_BG: &str = "\x1b[48;2;40;40;50m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Eq, PartialEq)]
pub struct TabRow {
    pub active: bool,
    pub path_label: String,
    pub command: Option<String>,
    pub is_agent: bool,
    pub is_busy: bool,
    pub git: GitStat,
}

impl TabRow {
    pub fn new(
        tab: &TabInfo,
        focused: Option<&PaneData>,
        priority_cmd: Option<(&str, bool)>,
        git: GitStat,
        home: &Path,
    ) -> Self {
        let path_label = focused
            .and_then(|e| e.cwd.as_ref())
            .map_or_else(|| tab.name.clone(), |p| short_path(p, home));

        let (command, is_agent, is_busy) = if let Some((name, busy)) = priority_cmd {
            (Some(name.to_owned()), true, busy)
        } else {
            (focused.and_then(|e| e.command.clone()), false, false)
        };

        Self {
            active: tab.active,
            path_label,
            command,
            is_agent,
            is_busy,
            git,
        }
    }

    fn write_path_lines(&self, buf: &mut String, y: &mut usize, width: usize, sep_col: usize) {
        let path_with_indent = format!("{MARKER}{}", self.path_label);
        let bg = if self.active { ACTIVE_BG } else { "" };
        let dim = if self.active { "" } else { DIM };

        for line in wrap_lines(&path_with_indent, width) {
            let row = *y + 1;
            let padded = pad(&line, width);
            let _ = write!(buf, "\x1b[{row};1H{bg}{dim}{padded}{RESET}");
            write_separator(buf, row, sep_col);
            *y += 1;
        }
    }

    fn write_info_line(&self, buf: &mut String, row_1based: usize, width: usize) {
        let bg = if self.active { ACTIVE_BG } else { "" };
        let fg = if self.active { "\x1b[39m" } else { DIM };

        let left = self.command.as_ref().map_or_else(String::new, |cmd| {
            if self.is_agent {
                let indicator_color = if self.is_busy { AMBER } else { DIM };
                let indicator = if self.is_busy { "\u{25cf}" } else { "\u{25cb}" };
                format!(" {indicator_color}{indicator}{bg}{fg} {cmd}")
            } else {
                format!(" {cmd}")
            }
        });

        let mut stats: Vec<(&str, String)> = Vec::new();
        if self.git.is_worktree {
            stats.push((CYAN, "W".into()));
        }
        if self.git.insertions > 0 {
            stats.push((GREEN, format!("+{}", self.git.insertions)));
        }
        if self.git.deletions > 0 {
            stats.push((RED, format!("-{}", self.git.deletions)));
        }
        if self.git.new_files > 0 {
            stats.push((CYAN, format!("?{}", self.git.new_files)));
        }

        let right_visible_len: usize =
            stats.iter().map(|(_, s)| s.chars().count()).sum::<usize>() + stats.len().saturating_sub(1);

        let left_visible_len = visible_len(&left);
        let gap = width.saturating_sub(left_visible_len + right_visible_len);

        let _ = write!(buf, "\x1b[{row_1based};1H{bg}{fg}{left}");
        for _ in 0..gap {
            buf.push(' ');
        }
        for (i, (color, text)) in stats.iter().enumerate() {
            if i > 0 {
                buf.push(' ');
            }
            buf.push_str(color);
            buf.push_str(text);
            buf.push_str(fg);
        }
        buf.push_str(RESET);
    }
}

pub fn render_frame(frame: &[TabRow], rows: usize, cols: usize, buf: &mut String) {
    if cols < 2 {
        return;
    }
    let content_w = cols - 1;
    let sep_col = cols;

    let mut y = 0;
    for entry in frame {
        let path_height = wrap_lines(&format!("{MARKER}{}", entry.path_label), content_w).len();
        let total = path_height + INFO_ROWS;
        if y + total > rows {
            break;
        }
        entry.write_path_lines(buf, &mut y, content_w, sep_col);
        entry.write_info_line(buf, y + 1, content_w);
        write_separator(buf, y + 1, sep_col);
        y += 1;
    }

    for row in y..rows {
        let r = row + 1;
        let blank = pad("", content_w);
        let _ = write!(buf, "\x1b[{r};1H{blank}");
        write_separator(buf, r, sep_col);
    }
}

pub fn tab_index_at_row(frame: &[TabRow], click_row: usize, content_w: usize) -> Option<usize> {
    let mut y = 0;
    for (i, entry) in frame.iter().enumerate() {
        let path_with_indent = format!("{MARKER}{}", entry.path_label);
        let height = wrap_lines(&path_with_indent, content_w).len() + INFO_ROWS;
        if click_row < y + height {
            return Some(i);
        }
        y += height;
    }
    None
}

fn write_separator(buf: &mut String, row_1based: usize, col: usize) {
    let _ = write!(buf, "\x1b[{row_1based};{col}H{SEP_COLOR}{SEPARATOR}{RESET}");
}

/// Count visible characters, skipping ANSI escape sequences.
fn visible_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

fn short_path(path: &Path, home: &Path) -> String {
    if path == home {
        return "~".into();
    }
    if let Ok(rel) = path.strip_prefix(home) {
        return format!("~/{}", rel.display());
    }
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + width).min(chars.len());
        lines.push(chars[start..end].iter().collect());
        start = end;
    }
    lines
}

fn pad(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= width {
        return chars[..width].iter().collect();
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
