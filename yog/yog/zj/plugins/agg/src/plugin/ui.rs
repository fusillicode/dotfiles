use agg::Cmd;
use agg::GitStat;
use agg::TabIndicator;

pub const BOLD: &str = "\x1b[1m";
pub const AGENT_WAITING_UNSEEN_FG: &str = "\x1b[38;2;255;0;0m";
pub const AGENT_BUSY_FG: &str = "\x1b[38;2;255;170;51m";
pub const GIT_NEW_LINES_FG: &str = "\x1b[38;2;140;228;121m";
pub const GIT_DEL_LINES_FG: &str = "\x1b[38;2;236;99;92m";
pub const GIT_NEW_FILES_FG: &str = "\x1b[38;2;0;255;255m";
pub const RESET: &str = "\x1b[0m";

pub fn agent_dot(indicator: TabIndicator, bg: &str, fg: &str) -> Option<String> {
    match indicator {
        TabIndicator::Unseen => Some(format!("{BOLD}{AGENT_WAITING_UNSEEN_FG}•{RESET}{bg}{fg}")),
        TabIndicator::Busy => Some(format!("{BOLD}{AGENT_BUSY_FG}•{RESET}{bg}{fg}")),
        TabIndicator::NoAgent | TabIndicator::Seen => None,
    }
}

pub fn cmd_label(cmd: &Cmd) -> &str {
    match cmd {
        Cmd::None => "",
        Cmd::Running(cmd) => cmd,
        Cmd::Agent { agent, .. } => agent.short_name(),
    }
}

pub fn display_left(indicator: TabIndicator, cmd: &Cmd, bg: &str, fg: &str) -> String {
    let dot = crate::plugin::ui::agent_dot(indicator, bg, fg);
    let label = crate::plugin::ui::cmd_label(cmd);
    match dot {
        Some(dot) if label.is_empty() => dot,
        Some(dot) => format!("{dot} {label}"),
        None => label.to_string(),
    }
}

pub fn git_stat_parts(git_stat: &GitStat) -> Vec<(&'static str, String)> {
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

pub fn visible_len(s: &str) -> usize {
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

pub fn pad(s: &str, width: usize) -> String {
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

pub fn pad_ansi(s: &str, width: usize) -> String {
    let len = visible_len(s);
    if len >= width {
        return s.to_string();
    }
    let mut out = String::from(s);
    for _ in len..width {
        out.push(' ');
    }
    out
}
