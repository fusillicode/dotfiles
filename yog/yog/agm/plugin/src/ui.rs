use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;

use agm_core::Cmd;
use agm_core::GitStat;
use zellij_tile::prelude::*;

const INFO_ROWS: usize = 1;
const MARKER: &str = " ";
const SEPARATOR: char = '\u{2502}';

const SEP_COLOR: &str = "\x1b[38;2;34;34;34m";
const GREEN: &str = "\x1b[38;2;0;255;0m";
const RED: &str = "\x1b[38;2;255;0;0m";
const CYAN: &str = "\x1b[38;2;0;255;255m";
const DIM: &str = "\x1b[38;2;100;100;110m";
const AMBER: &str = "\x1b[38;2;255;170;62m";
const ACTIVE_BG: &str = "\x1b[48;2;40;40;50m";
const DEFAULT_FG: &str = "\x1b[39m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Eq, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub struct TabRow {
    pub active: bool,
    pub path_label: String,
    pub cmd: Cmd,
    pub git: GitStat,
}

impl TabRow {
    pub fn new(tab: &TabInfo, cwd: Option<&PathBuf>, cmd: Cmd, git: GitStat, home: &Path) -> Self {
        let path_label = cwd.map_or_else(|| tab.name.clone(), |path| short_path(path, home));
        Self {
            active: tab.active,
            path_label,
            cmd,
            git,
        }
    }

    fn write_path_lines(&self, buf: &mut String, y: &mut usize, width: usize, sep_col: usize) {
        let path_with_indent = format!("{MARKER}{}", self.path_label);
        let (bg, dim) = if self.active { (ACTIVE_BG, "") } else { ("", DIM) };

        for line in wrap_lines(&path_with_indent, width) {
            let row = *y + 1;
            let padded = pad(&line, width);
            let _ = write!(buf, "\x1b[{row};1H{bg}{dim}{padded}{RESET}");
            write_separator(buf, row, sep_col);
            *y += 1;
        }
    }

    fn write_info_line(&self, buf: &mut String, row_1based: usize, width: usize) {
        let (bg, fg) = if self.active {
            (ACTIVE_BG, DEFAULT_FG)
        } else {
            ("", DIM)
        };

        let left = match &self.cmd {
            Cmd::None => String::new(),
            Cmd::Running(cmd) => format!(" {cmd}"),
            Cmd::IdleAgent(agent) => {
                format!(" {DIM}○ {bg}{fg} {}", agent.name())
            }
            Cmd::BusyAgent(agent) => {
                format!(" {AMBER}● {bg}{fg} {}", agent.name())
            }
        };

        let mut stats: Vec<(&str, String)> = Vec::new();
        if self.git.is_worktree {
            stats.push((CYAN, "[W]".into()));
        }
        if self.git.insertions > 0 {
            stats.push((GREEN, format!("+{}", self.git.insertions)));
        }
        if self.git.deletions > 0 {
            stats.push((RED, format!("-{}", self.git.deletions)));
        }
        if self.git.new_files > 0 {
            stats.push((AMBER, format!("?{}", self.git.new_files)));
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use agm_core::Agent;
    use agm_core::Cmd;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_tab_row_new_active_with_path() {
        let tab = TabInfo {
            name: "test".to_string(),
            active: true,
            position: 0,
            ..Default::default()
        };

        let cwd = Some(PathBuf::from("/home/user/project"));
        let git = GitStat::default();
        let home = Path::new("/home");

        let expected = TabRow {
            active: true,
            path_label: "user/project".to_string(),
            cmd: Cmd::Running("nvim".to_string()),
            git: GitStat::default(),
        };
        let actual = TabRow::new(&tab, cwd.as_ref(), Cmd::Running("nvim".to_string()), git, home);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tab_row_new_inactive_with_home_dir() {
        let tab = TabInfo {
            name: "shell".to_string(),
            active: false,
            position: 1,
            ..Default::default()
        };

        let cwd = Some(PathBuf::from("/home/user"));
        let git = GitStat::default();
        let home = Path::new("/home");

        let expected = TabRow {
            active: false,
            path_label: "user".to_string(),
            cmd: Cmd::Running("zsh".to_string()),
            git: GitStat::default(),
        };
        let actual = TabRow::new(&tab, cwd.as_ref(), Cmd::Running("zsh".to_string()), git, home);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tab_row_new_with_priority_command() {
        let tab = TabInfo {
            name: "agent-tab".to_string(),
            active: true,
            position: 2,
            ..Default::default()
        };

        let git = GitStat::default();
        let home = Path::new("/");

        let expected = TabRow {
            active: true,
            path_label: "agent-tab".to_string(),
            cmd: Cmd::BusyAgent(Agent::Claude),
            git: GitStat::default(),
        };
        let actual = TabRow::new(&tab, None, Cmd::BusyAgent(Agent::Claude), git, home);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tab_row_new_with_no_focused_pane() {
        let tab = TabInfo {
            name: "empty".to_string(),
            active: true,
            position: 3,
            ..Default::default()
        };

        let git = GitStat::default();
        let home = Path::new("/tmp");

        let expected = TabRow {
            active: true,
            path_label: "empty".to_string(),
            cmd: Cmd::None,
            git: GitStat::default(),
        };
        let actual = TabRow::new(&tab, None, Cmd::None, git, home);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tab_row_new_git_stat_copied() {
        let git = GitStat {
            insertions: 5,
            deletions: 3,
            ..Default::default()
        };

        let tab = TabInfo {
            name: "git-tab".to_string(),
            active: true,
            position: 4,
            ..Default::default()
        };

        let home = Path::new("/");

        let expected = TabRow {
            active: true,
            path_label: "git-tab".to_string(),
            cmd: Cmd::None,
            git,
        };
        let actual = TabRow::new(&tab, None, Cmd::None, git, home);
        assert_eq!(actual, expected);
    }
}
