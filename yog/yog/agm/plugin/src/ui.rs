use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;

use agm_core::Cmd;
use agm_core::TabIndicator;
use agm_core::git_stat::GitStat;
use zellij_tile::prelude::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabRow {
    pub active: bool,
    pub path_label: String,
    pub cmd: Cmd,
    pub indicator: TabIndicator,
    pub git: GitStat,
}

impl TabRow {
    pub fn new(
        tab: &TabInfo,
        cwd: Option<&PathBuf>,
        cmd: Cmd,
        indicator: TabIndicator,
        git: GitStat,
        home: &Path,
    ) -> Self {
        let path_label = cwd.map_or_else(|| tab.name.clone(), |path| agm_core::short_path(path, home));
        Self {
            active: tab.active,
            path_label,
            cmd,
            indicator,
            git,
        }
    }

    fn write_path_lines(&self, buf: &mut String, y: &mut usize, content_w: usize, sep_col: usize) {
        let inner_w = tab_inner_width(content_w);
        let path_lines = wrap_lines(&self.path_label, inner_w);
        let bg = TAB_INACTIVE_BG;
        let path_fg = if self.active { TAB_DEFAULT_FG } else { PATH_INACTIVE_FG };
        let rail = if self.active {
            format!("{RAIL_ACTIVE_FG}▎")
        } else {
            format!("{RAIL_INACTIVE_FG}▏")
        };

        for line in path_lines.iter() {
            let row = *y + 1;
            let prefix = if content_w >= 2 {
                if self.active {
                    format!("\x1b[{row};1H{bg}{rail}{bg}{BOLD}")
                } else {
                    format!("\x1b[{row};1H{bg}{rail}{bg}")
                }
            } else {
                if self.active {
                    format!("\x1b[{row};1H{bg}{BOLD}")
                } else {
                    format!("\x1b[{row};1H{bg}")
                }
            };
            buf.push_str(&prefix);

            {
                let padded = pad(line, inner_w);
                let _ = write!(buf, "{path_fg}{padded}{RESET}");
            }

            write_separator(buf, row, sep_col);
            *y += 1;
        }
    }

    fn write_blank_line(&self, buf: &mut String, row_1based: usize, content_w: usize, sep_col: usize) {
        let inner_w = tab_inner_width(content_w);
        let bg = TAB_INACTIVE_BG;
        let blank = pad("", inner_w);
        if content_w >= 2 {
            let (rail_color, rail_char) = if self.active {
                (RAIL_ACTIVE_FG, '▎')
            } else {
                (RAIL_INACTIVE_FG, '▏')
            };
            let _ = write!(buf, "\x1b[{row_1based};1H{bg}{rail_color}{rail_char}{bg}{blank}{RESET}");
        } else {
            let _ = write!(buf, "\x1b[{row_1based};1H{bg}{blank}{RESET}");
        }
        write_separator(buf, row_1based, sep_col);
    }

    fn write_info_line(&self, buf: &mut String, row_1based: usize, content_w: usize, sep_col: usize) {
        let inner_w = tab_inner_width(content_w);
        let bg = TAB_INACTIVE_BG;
        let cmd_fg = if self.active { TAB_DEFAULT_FG } else { PATH_INACTIVE_FG };

        let left = display_left(self.indicator, &self.cmd, bg, cmd_fg);

        let stats = format_git_stat_parts(&self.git);
        let stats_vis: usize =
            stats.iter().map(|(_, s)| s.chars().count()).sum::<usize>() + stats.len().saturating_sub(1);

        let left_vis = visible_len(&left);
        let gap = inner_w.saturating_sub(left_vis + stats_vis);

        let rail = if content_w >= 2 {
            if self.active {
                format!("{bg}{RAIL_ACTIVE_FG}▎")
            } else {
                format!("{bg}{RAIL_INACTIVE_FG}▏")
            }
        } else {
            String::new()
        };

        let _ = write!(buf, "\x1b[{row_1based};1H{rail}{bg}{cmd_fg}{left}");
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
        let path_height = wrap_lines(&entry.path_label, inner_w).len();
        let total = path_height + INFO_ROWS + 1;
        if y + total > rows {
            break;
        }
        entry.write_path_lines(buf, &mut y, content_w, sep_col);
        entry.write_info_line(buf, y + 1, content_w, sep_col);
        y += 1;
        entry.write_blank_line(buf, y + 1, content_w, sep_col);
        y += 1;
    }

    for row in y..rows {
        let r = row + 1;
        let blank = pad("", content_w);
        let _ = write!(buf, "\x1b[{r};1H{TAB_INACTIVE_BG}{blank}{RESET}");
        write_separator(buf, r, sep_col);
    }
}

pub fn tab_index_at_row(frame: &[TabRow], click_row: usize, content_w: usize) -> Option<usize> {
    let mut y = 0;
    for (i, entry) in frame.iter().enumerate() {
        let inner_w = tab_inner_width(content_w);
        let height = wrap_lines(&entry.path_label, inner_w).len() + INFO_ROWS + 1;
        if click_row < y + height {
            return Some(i);
        }
        y += height;
    }
    None
}

const INFO_ROWS: usize = 1;
const BOLD: &str = "\x1b[1m";
const SEPARATOR: char = '\u{2502}';

const SEP_COLOR: &str = "\x1b[38;2;50;50;50m";
const GIT_NEW_LINES_FG: &str = "\x1b[38;2;140;228;121m";
const GIT_DEL_LINES_FG: &str = "\x1b[38;2;236;99;92m";
const GIT_NEW_FILES_FG: &str = "\x1b[38;2;0;255;255m";
const AGENT_WAITING_SEEN_FG: &str = "\x1b[38;2;100;100;110m";
const AGENT_WAITING_UNSEEN_FG: &str = "\x1b[38;2;255;0;0m";
const AGENT_BUSY_FG: &str = "\x1b[38;2;255;170;51m";
const TAB_INACTIVE_BG: &str = "\x1b[48;2;0;0;0m";
const TAB_DEFAULT_FG: &str = "\x1b[39m";
const PATH_INACTIVE_FG: &str = "\x1b[38;2;119;119;119m";
const RAIL_ACTIVE_FG: &str = "\x1b[38;2;106;106;223m";
const RAIL_INACTIVE_FG: &str = "\x1b[38;2;0;0;0m";
const RESET: &str = "\x1b[0m";

fn display_left(indicator: TabIndicator, cmd: &Cmd, bg: &str, fg: &str) -> String {
    let dot = match indicator {
        TabIndicator::None => None,
        TabIndicator::Red => Some(format!("{AGENT_WAITING_UNSEEN_FG}•")),
        TabIndicator::Green => Some(format!("{AGENT_BUSY_FG}•")),
        TabIndicator::Empty => Some(format!("{AGENT_WAITING_SEEN_FG}•")),
    };
    let label = match cmd {
        Cmd::None => String::new(),
        Cmd::Running(cmd) => cmd.clone(),
        Cmd::Agent { agent, .. } => agent.name().to_string(),
    };

    match dot {
        Some(dot) if label.is_empty() => format!("{dot}{bg}{fg}"),
        Some(dot) => format!("{dot} {bg}{fg}{label}"),
        None => label,
    }
}

/// Compact tokens; `write_info_line` inserts exactly one ASCII space between each part.
fn format_git_stat_parts(git_stat: &GitStat) -> Vec<(&'static str, String)> {
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

    use agm_core::AgentState;
    use agm_core::Cmd;
    use agm_core::agent::Agent;
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
            indicator: TabIndicator::None,
            git: GitStat::default(),
        };
        let actual = TabRow::new(
            &tab,
            cwd.as_ref(),
            Cmd::Running("nvim".to_string()),
            TabIndicator::None,
            git,
            home,
        );
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
            indicator: TabIndicator::None,
            git: GitStat::default(),
        };
        let actual = TabRow::new(
            &tab,
            cwd.as_ref(),
            Cmd::Running("zsh".to_string()),
            TabIndicator::None,
            git,
            home,
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_write_path_lines_active_uses_bold() {
        let entry = TabRow {
            active: true,
            path_label: "tab".to_string(),
            cmd: Cmd::None,
            indicator: TabIndicator::None,
            git: GitStat::default(),
        };
        let mut actual = String::new();
        let mut y = 0;

        entry.write_path_lines(&mut actual, &mut y, 4, 5);

        assert_eq!(
            actual,
            format!(
                "\x1b[1;1H{TAB_INACTIVE_BG}{RAIL_ACTIVE_FG}▎{TAB_INACTIVE_BG}{BOLD}{TAB_DEFAULT_FG}tab{RESET}\x1b[1;5H{SEP_COLOR}{SEPARATOR}{RESET}"
            )
        );
    }

    #[test]
    fn test_write_path_lines_inactive_skips_bold() {
        let entry = TabRow {
            active: false,
            path_label: "tab".to_string(),
            cmd: Cmd::None,
            indicator: TabIndicator::None,
            git: GitStat::default(),
        };
        let mut actual = String::new();
        let mut y = 0;

        entry.write_path_lines(&mut actual, &mut y, 4, 5);

        assert_eq!(
            actual,
            format!(
                "\x1b[1;1H{TAB_INACTIVE_BG}{RAIL_INACTIVE_FG}▏{TAB_INACTIVE_BG}{PATH_INACTIVE_FG}tab{RESET}\x1b[1;5H{SEP_COLOR}{SEPARATOR}{RESET}"
            )
        );
        assert!(!actual.contains(BOLD));
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
            cmd: Cmd::agent(Agent::Claude, AgentState::Busy),
            indicator: TabIndicator::Green,
            git: GitStat::default(),
        };
        let actual = TabRow::new(
            &tab,
            None,
            Cmd::agent(Agent::Claude, AgentState::Busy),
            TabIndicator::Green,
            git,
            home,
        );
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
            indicator: TabIndicator::None,
            git: GitStat::default(),
        };
        let actual = TabRow::new(&tab, None, Cmd::None, TabIndicator::None, git, home);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_display_left_waiting_unseen_uses_small_red_dot() {
        let rendered = display_left(
            TabIndicator::Red,
            &Cmd::agent(Agent::Codex, AgentState::Acknowledged),
            TAB_INACTIVE_BG,
            TAB_DEFAULT_FG,
        );
        assert_eq!(
            rendered,
            format!("{AGENT_WAITING_UNSEEN_FG}• {TAB_INACTIVE_BG}{TAB_DEFAULT_FG}codex")
        );
    }

    #[test]
    fn test_display_left_waiting_seen_uses_small_gray_dot() {
        let rendered = display_left(
            TabIndicator::Empty,
            &Cmd::agent(Agent::Codex, AgentState::Acknowledged),
            TAB_INACTIVE_BG,
            TAB_DEFAULT_FG,
        );
        assert_eq!(
            rendered,
            format!("{AGENT_WAITING_SEEN_FG}• {TAB_INACTIVE_BG}{TAB_DEFAULT_FG}codex")
        );
    }

    #[test]
    fn test_display_left_busy_uses_small_green_dot() {
        let rendered = display_left(
            TabIndicator::Green,
            &Cmd::agent(Agent::Codex, AgentState::Acknowledged),
            TAB_INACTIVE_BG,
            TAB_DEFAULT_FG,
        );
        assert_eq!(
            rendered,
            format!("{AGENT_BUSY_FG}• {TAB_INACTIVE_BG}{TAB_DEFAULT_FG}codex")
        );
    }

    #[test]
    fn test_display_left_none_indicator_renders_only_running_cmd_label() {
        let rendered = display_left(
            TabIndicator::None,
            &Cmd::Running("cargo".to_string()),
            TAB_INACTIVE_BG,
            TAB_DEFAULT_FG,
        );
        assert_eq!(rendered, "cargo");
    }

    #[test]
    fn test_display_left_none_indicator_renders_nothing_for_empty_cmd() {
        let rendered = display_left(TabIndicator::None, &Cmd::None, TAB_INACTIVE_BG, TAB_DEFAULT_FG);
        assert_eq!(rendered, "");
    }

    #[test]
    fn test_short_path_under_home() {
        let home = Path::new("/home/user");
        assert_eq!(
            agm_core::short_path(Path::new("/home/user/src/pkg/myproject"), home),
            "~/s/p/myproject"
        );
    }

    #[test]
    fn test_short_path_many_dirs() {
        let home = Path::new("/home/user");
        assert_eq!(
            agm_core::short_path(Path::new("/home/user/one/two/three/four/five"), home),
            "~/o/t/t/f/five"
        );
    }

    #[test]
    fn test_short_path_outside_home() {
        let home = Path::new("/home/user");
        assert_eq!(agm_core::short_path(Path::new("/opt/pkg/foo"), home), "/o/p/foo");
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
            indicator: TabIndicator::None,
            git,
        };
        let actual = TabRow::new(&tab, None, Cmd::None, TabIndicator::None, git, home);
        assert_eq!(actual, expected);
    }
}
