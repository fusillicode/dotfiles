use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;

use agg::Cmd;
use agg::GitStat;
use agg::TabIndicator;
use zellij_tile::prelude::*;

const INFO_ROWS: usize = 1;
const SEPARATOR: char = '\u{2502}';

const SEP_COLOR: &str = "\x1b[38;2;50;50;50m";
const TAB_INACTIVE_BG: &str = "\x1b[48;2;0;19;0m";
const TAB_DEFAULT_FG: &str = "\x1b[39m";
const PATH_INACTIVE_FG: &str = "\x1b[38;2;119;119;119m";
const RAIL_ACTIVE_FG: &str = "\x1b[38;2;106;106;223m";
const RAIL_INACTIVE_FG: &str = "\x1b[38;2;0;19;0m";

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
        let path_label = cwd.map_or_else(|| String::from("-"), |path| ytil_tui::short_path(path, home));
        Self {
            active: tab.active,
            path_label,
            cmd,
            indicator,
            git,
        }
    }

    pub fn placeholder(tab: &TabInfo) -> Self {
        Self {
            active: tab.active,
            path_label: tab.name.clone(),
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            git: GitStat::default(),
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

        for line in &path_lines {
            let row = y.saturating_add(1);
            let prefix = if content_w >= 2 {
                if self.active {
                    format!("\x1b[{row};1H{bg}{rail}{bg}{}", crate::plugin::ui::BOLD)
                } else {
                    format!("\x1b[{row};1H{bg}{rail}{bg}")
                }
            } else {
                if self.active {
                    format!("\x1b[{row};1H{bg}{}", crate::plugin::ui::BOLD)
                } else {
                    format!("\x1b[{row};1H{bg}")
                }
            };
            buf.push_str(&prefix);

            {
                let padded = crate::plugin::ui::pad(line, inner_w);
                let _ = write!(buf, "{path_fg}{padded}{}", crate::plugin::ui::RESET);
            }

            write_separator(buf, row, sep_col);
            *y = y.saturating_add(1);
        }
    }

    fn write_blank_line(&self, buf: &mut String, row_1based: usize, content_w: usize, sep_col: usize) {
        let inner_w = tab_inner_width(content_w);
        let bg = TAB_INACTIVE_BG;
        let blank = crate::plugin::ui::pad("", inner_w);
        if content_w >= 2 {
            let (rail_color, rail_char) = if self.active {
                (RAIL_ACTIVE_FG, '▎')
            } else {
                (RAIL_INACTIVE_FG, '▏')
            };
            let _ = write!(
                buf,
                "\x1b[{row_1based};1H{bg}{rail_color}{rail_char}{bg}{blank}{}",
                crate::plugin::ui::RESET
            );
        } else {
            let _ = write!(buf, "\x1b[{row_1based};1H{bg}{blank}{}", crate::plugin::ui::RESET);
        }
        write_separator(buf, row_1based, sep_col);
    }

    fn write_info_line(&self, buf: &mut String, row_1based: usize, content_w: usize, sep_col: usize) {
        let inner_w = tab_inner_width(content_w);
        let bg = TAB_INACTIVE_BG;
        let cmd_fg = if self.active { TAB_DEFAULT_FG } else { PATH_INACTIVE_FG };

        let left = crate::plugin::ui::display_left(self.indicator, &self.cmd, bg, cmd_fg);

        let stats = crate::plugin::ui::git_stat_parts(&self.git);
        let stats_vis = stats
            .iter()
            .map(|(_, s)| s.chars().count())
            .sum::<usize>()
            .saturating_add(stats.len().saturating_sub(1));

        let left_vis = crate::plugin::ui::visible_len(&left);
        let gap = inner_w.saturating_sub(left_vis.saturating_add(stats_vis));

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
        buf.push_str(crate::plugin::ui::RESET);
        write_separator(buf, row_1based, sep_col);
    }
}

pub fn render_frame(frame: &[TabRow], rows: usize, cols: usize, buf: &mut String) {
    if cols < 2 {
        return;
    }
    let content_w = cols.saturating_sub(1);
    let sep_col = cols;

    let mut y = 0_usize;
    for entry in frame {
        let inner_w = tab_inner_width(content_w);
        let path_height = wrap_lines(&entry.path_label, inner_w).len();
        let total = path_height.saturating_add(INFO_ROWS).saturating_add(1);
        if y.saturating_add(total) > rows {
            break;
        }
        entry.write_path_lines(buf, &mut y, content_w, sep_col);
        entry.write_info_line(buf, y.saturating_add(1), content_w, sep_col);
        y = y.saturating_add(1);
        entry.write_blank_line(buf, y.saturating_add(1), content_w, sep_col);
        y = y.saturating_add(1);
    }

    for row in y..rows {
        let r = row.saturating_add(1);
        let blank = crate::plugin::ui::pad("", content_w);
        let _ = write!(buf, "\x1b[{r};1H{TAB_INACTIVE_BG}{blank}{}", crate::plugin::ui::RESET);
        write_separator(buf, r, sep_col);
    }
}

pub fn tab_index_at_row(frame: &[TabRow], click_row: usize, content_w: usize) -> Option<usize> {
    let mut y = 0_usize;
    for (i, entry) in frame.iter().enumerate() {
        let inner_w = tab_inner_width(content_w);
        let height = wrap_lines(&entry.path_label, inner_w)
            .len()
            .saturating_add(INFO_ROWS)
            .saturating_add(1);
        if click_row < y.saturating_add(height) {
            return Some(i);
        }
        y = y.saturating_add(height);
    }
    None
}

/// Text width inside the content area: one column reserved for the left rail when `content_w >= 2`.
fn tab_inner_width(content_w: usize) -> usize {
    if content_w >= 2 {
        content_w.saturating_sub(1).max(1)
    } else {
        content_w.max(1)
    }
}

fn write_separator(buf: &mut String, row_1based: usize, col: usize) {
    let _ = write!(
        buf,
        "\x1b[{row_1based};{col}H{SEP_COLOR}{SEPARATOR}{}",
        crate::plugin::ui::RESET
    );
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
    let mut line = String::new();
    for ch in chars {
        if line.chars().count() == width {
            lines.push(std::mem::take(&mut line));
        }
        line.push(ch);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use agg::AgentState;
    use agg::Cmd;
    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;

    use crate::plugin::tab_bar::ui::*;

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
            indicator: TabIndicator::NoAgent,
            git: GitStat::default(),
        };
        let actual = TabRow::new(
            &tab,
            cwd.as_ref(),
            Cmd::Running("nvim".to_string()),
            TabIndicator::NoAgent,
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
            indicator: TabIndicator::NoAgent,
            git: GitStat::default(),
        };
        let actual = TabRow::new(
            &tab,
            cwd.as_ref(),
            Cmd::Running("zsh".to_string()),
            TabIndicator::NoAgent,
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
            indicator: TabIndicator::NoAgent,
            git: GitStat::default(),
        };
        let mut actual = String::new();
        let mut y = 0;

        entry.write_path_lines(&mut actual, &mut y, 4, 5);

        assert_eq!(
            actual,
            format!(
                "\x1b[1;1H{TAB_INACTIVE_BG}{RAIL_ACTIVE_FG}▎{TAB_INACTIVE_BG}{}{TAB_DEFAULT_FG}tab{}\x1b[1;5H{SEP_COLOR}{SEPARATOR}{}",
                crate::plugin::ui::BOLD,
                crate::plugin::ui::RESET,
                crate::plugin::ui::RESET
            )
        );
    }

    #[test]
    fn test_write_path_lines_inactive_skips_bold() {
        let entry = TabRow {
            active: false,
            path_label: "tab".to_string(),
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            git: GitStat::default(),
        };
        let mut actual = String::new();
        let mut y = 0;

        entry.write_path_lines(&mut actual, &mut y, 4, 5);

        assert_eq!(
            actual,
            format!(
                "\x1b[1;1H{TAB_INACTIVE_BG}{RAIL_INACTIVE_FG}▏{TAB_INACTIVE_BG}{PATH_INACTIVE_FG}tab{}\x1b[1;5H{SEP_COLOR}{SEPARATOR}{}",
                crate::plugin::ui::RESET,
                crate::plugin::ui::RESET
            )
        );
        assert!(!actual.contains(crate::plugin::ui::BOLD));
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
            path_label: "-".to_string(),
            cmd: Cmd::agent(Agent::Claude, AgentState::Busy),
            indicator: TabIndicator::Busy,
            git: GitStat::default(),
        };
        let actual = TabRow::new(
            &tab,
            None,
            Cmd::agent(Agent::Claude, AgentState::Busy),
            TabIndicator::Busy,
            git,
            home,
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_tab_row_placeholder_uses_tab_name() {
        let tab = TabInfo {
            name: "blank".to_string(),
            active: true,
            position: 3,
            ..Default::default()
        };

        let expected = TabRow {
            active: true,
            path_label: "blank".to_string(),
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            git: GitStat::default(),
        };
        let actual = TabRow::placeholder(&tab);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_display_left_unseen_uses_bold_attention_dot() {
        let rendered = crate::plugin::ui::display_left(
            TabIndicator::Unseen,
            &Cmd::agent(Agent::Codex, AgentState::Acknowledged),
            TAB_INACTIVE_BG,
            TAB_DEFAULT_FG,
        );
        assert_eq!(
            rendered,
            format!(
                "{}{}•{}{TAB_INACTIVE_BG}{TAB_DEFAULT_FG} cx",
                crate::plugin::ui::BOLD,
                crate::plugin::ui::AGENT_WAITING_UNSEEN_FG,
                crate::plugin::ui::RESET
            )
        );
    }

    #[test]
    fn test_display_left_seen_renders_only_agent_name() {
        let rendered = crate::plugin::ui::display_left(
            TabIndicator::Seen,
            &Cmd::agent(Agent::Codex, AgentState::Acknowledged),
            TAB_INACTIVE_BG,
            TAB_DEFAULT_FG,
        );
        assert_eq!(rendered, format!("cx"));
    }

    #[test]
    fn test_display_left_busy_uses_bold_dot() {
        let rendered = crate::plugin::ui::display_left(
            TabIndicator::Busy,
            &Cmd::agent(Agent::Codex, AgentState::Acknowledged),
            TAB_INACTIVE_BG,
            TAB_DEFAULT_FG,
        );
        assert_eq!(
            rendered,
            format!(
                "{}{}•{}{TAB_INACTIVE_BG}{TAB_DEFAULT_FG} cx",
                crate::plugin::ui::BOLD,
                crate::plugin::ui::AGENT_BUSY_FG,
                crate::plugin::ui::RESET
            )
        );
    }

    #[test]
    fn test_display_left_no_agent_indicator_renders_only_running_cmd_label() {
        let rendered = crate::plugin::ui::display_left(
            TabIndicator::NoAgent,
            &Cmd::Running("cargo".to_string()),
            TAB_INACTIVE_BG,
            TAB_DEFAULT_FG,
        );
        assert_eq!(rendered, "cargo");
    }

    #[test]
    fn test_display_left_no_agent_indicator_renders_nothing_for_empty_cmd() {
        let rendered =
            crate::plugin::ui::display_left(TabIndicator::NoAgent, &Cmd::None, TAB_INACTIVE_BG, TAB_DEFAULT_FG);
        assert_eq!(rendered, "");
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
            path_label: "-".to_string(),
            cmd: Cmd::None,
            indicator: TabIndicator::NoAgent,
            git: git.clone(),
        };
        let actual = TabRow::new(&tab, None, Cmd::None, TabIndicator::NoAgent, git, home);
        assert_eq!(actual, expected);
    }
}
