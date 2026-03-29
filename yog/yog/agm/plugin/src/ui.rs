use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;

use agm_core::Cmd;
use agm_core::GitStat;
use zellij_tile::prelude::*;

const INFO_ROWS: usize = 1;
const BOLD: &str = "\x1b[1m";
const SEPARATOR: char = '\u{2502}';

const SEP_COLOR: &str = "\x1b[38;2;34;34;34m";
const GREEN: &str = "\x1b[38;2;0;255;0m";
const RED: &str = "\x1b[38;2;255;0;0m";
const CYAN: &str = "\x1b[38;2;0;255;255m";
const DIM: &str = "\x1b[38;2;100;100;110m";
/// Bold, saturated amber — agent busy (same hue family as before, higher chroma).
const BUSY_AGENT_FG: &str = "\x1b[1;38;2;255;200;0m";
/// Focused tab: slightly lifted from the pane background.
const ACTIVE_BG: &str = "\x1b[48;2;52;52;68m";
/// Unfocused tab: same as default black pane.
const INACTIVE_BG: &str = "\x1b[48;2;0;0;0m";
const DEFAULT_FG: &str = "\x1b[39m";
/// Inactive path/command text on black.
const PATH_INACTIVE_FG: &str = "\x1b[38;2;142;145;160m";
const RAIL_ACTIVE_FG: &str = "\x1b[38;2;190;150;255m";
const RAIL_INACTIVE_FG: &str = "\x1b[38;2;0;0;0m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabRow {
    pub active: bool,
    pub path_label: String,
    pub cmd: Cmd,
    pub git: GitStat,
}

impl TabRow {
    pub fn new(tab: &TabInfo, cwd: Option<&PathBuf>, cmd: Cmd, git: GitStat, home: &Path) -> Self {
        let path_label = cwd.map_or_else(|| tab.name.clone(), |path| sidebar_path(path, home));
        Self {
            active: tab.active,
            path_label,
            cmd,
            git,
        }
    }

    fn write_path_lines(&self, buf: &mut String, y: &mut usize, content_w: usize, sep_col: usize) {
        let inner_w = tab_inner_width(content_w);
        let path_line = path_line_for_wrap(self);
        let path_lines = wrap_lines(&path_line, inner_w);
        let (bg, path_fg) = if self.active {
            (ACTIVE_BG, DEFAULT_FG)
        } else {
            (INACTIVE_BG, PATH_INACTIVE_FG)
        };
        let rail = if self.active {
            format!("{RAIL_ACTIVE_FG}▎")
        } else {
            format!("{RAIL_INACTIVE_FG}▏")
        };

        for line in &path_lines {
            let row = *y + 1;
            let padded = pad(line, inner_w);
            if content_w >= 2 {
                let _ = write!(buf, "\x1b[{row};1H{bg}{rail}{bg}{BOLD}{path_fg}{padded}{RESET}");
            } else {
                let _ = write!(buf, "\x1b[{row};1H{bg}{BOLD}{path_fg}{padded}{RESET}");
            }
            write_separator(buf, row, sep_col);
            *y += 1;
        }
    }

    fn write_info_line(&self, buf: &mut String, row_1based: usize, content_w: usize, sep_col: usize) {
        let inner_w = tab_inner_width(content_w);
        let (bg, cmd_fg) = if self.active {
            (ACTIVE_BG, DEFAULT_FG)
        } else {
            (INACTIVE_BG, PATH_INACTIVE_FG)
        };

        let left = display_cmd(&self.cmd, bg, cmd_fg);
        let wt = if self.git.is_worktree { " [W]" } else { "" };
        let wt_vis = wt.chars().count();

        let stats = format_git_stat_parts(&self.git);
        let stats_vis: usize =
            stats.iter().map(|(_, s)| s.chars().count()).sum::<usize>() + stats.len().saturating_sub(1);

        let left_vis = visible_len(&left);
        let gap = inner_w.saturating_sub(left_vis + wt_vis + stats_vis);

        let rail = if content_w >= 2 {
            if self.active {
                format!("{bg}{RAIL_ACTIVE_FG}▎")
            } else {
                format!("{bg}{RAIL_INACTIVE_FG}▏")
            }
        } else {
            String::new()
        };

        let _ = write!(buf, "\x1b[{row_1based};1H{rail}{bg}{cmd_fg}{left}{wt}");
        for _ in 0..gap {
            buf.push(' ');
        }
        if !stats.is_empty() {
            for (i, (color, text)) in stats.iter().enumerate() {
                if i > 0 {
                    buf.push(' ');
                }
                buf.push_str(color);
                buf.push_str(text);
            }
        }
        buf.push_str(RESET);
        write_separator(buf, row_1based, sep_col);
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
        let inner_w = tab_inner_width(content_w);
        let path_height = wrap_lines(&path_line_for_wrap(entry), inner_w).len();
        let total = path_height + INFO_ROWS;
        if y + total > rows {
            break;
        }
        entry.write_path_lines(buf, &mut y, content_w, sep_col);
        entry.write_info_line(buf, y + 1, content_w, sep_col);
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
        let inner_w = tab_inner_width(content_w);
        let height = wrap_lines(&path_line_for_wrap(entry), inner_w).len() + INFO_ROWS;
        if click_row < y + height {
            return Some(i);
        }
        y += height;
    }
    None
}

fn display_cmd(cmd: &Cmd, bg: &str, fg: &str) -> String {
    match cmd {
        Cmd::None => String::new(),
        Cmd::Running(cmd) => format!(" {cmd}"),
        Cmd::IdleAgent(agent) => {
            format!(" {DIM}○ {bg}{fg}{}", agent.name())
        }
        Cmd::BusyAgent(agent) => {
            format!(" {BUSY_AGENT_FG}● {bg}{fg}{}", agent.name())
        }
    }
}

/// Compact tokens; `write_info_line` inserts exactly one ASCII space between each part.
fn format_git_stat_parts(git_stat: &GitStat) -> Vec<(&'static str, String)> {
    let mut stats = Vec::new();
    if git_stat.insertions > 0 {
        stats.push((GREEN, format!("+{}", git_stat.insertions)));
    }
    if git_stat.deletions > 0 {
        stats.push((RED, format!("-{}", git_stat.deletions)));
    }
    if git_stat.new_files > 0 {
        stats.push((CYAN, format!("?{}", git_stat.new_files)));
    }
    stats
}

fn path_line_for_wrap(entry: &TabRow) -> String {
    entry.path_label.clone()
}

/// Text width inside the content area: one column reserved for the left rail when `content_w >= 2`.
fn tab_inner_width(content_w: usize) -> usize {
    if content_w >= 2 {
        (content_w - 1).max(1)
    } else {
        content_w.max(1)
    }
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

fn path_dir_names(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Each parent directory → its first character; last segment → full name.
fn abbrev_path_dirs(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names[0].clone(),
        n => {
            let mut out = String::new();
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    out.push('/');
                }
                if i + 1 == n {
                    out.push_str(name);
                } else {
                    out.push(name.chars().next().unwrap_or('·'));
                }
            }
            out
        }
    }
}

/// Sidebar cwd string. Uses `~/…` when under `home` (unless `home` is `/`, which would match everything).
fn sidebar_path(path: &Path, home: &Path) -> String {
    if home != Path::new("/") {
        if path == home {
            return "~".into();
        }
        if let Ok(rel) = path.strip_prefix(home) {
            let names = path_dir_names(rel);
            return if names.is_empty() {
                "~".into()
            } else {
                format!("~/{}", abbrev_path_dirs(&names))
            };
        }
    }

    let names = path_dir_names(path);
    if names.is_empty() {
        "/".into()
    } else {
        format!("/{}", abbrev_path_dirs(&names))
    }
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
            path_label: "~/u/project".to_string(),
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
            path_label: "~/user".to_string(),
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
    fn sidebar_path_under_home() {
        let home = Path::new("/home/user");
        assert_eq!(
            sidebar_path(Path::new("/home/user/src/pkg/myproject"), home),
            "~/s/p/myproject"
        );
    }

    #[test]
    fn sidebar_path_many_dirs() {
        let home = Path::new("/home/user");
        assert_eq!(
            sidebar_path(Path::new("/home/user/one/two/three/four/five"), home),
            "~/o/t/t/f/five"
        );
    }

    #[test]
    fn sidebar_path_outside_home() {
        let home = Path::new("/home/user");
        assert_eq!(sidebar_path(Path::new("/opt/pkg/foo"), home), "/o/p/foo");
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
