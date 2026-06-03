use std::io::Write;

use crossterm::Command;
use crossterm::QueueableCommand;
use crossterm::cursor::MoveTo;
use crossterm::cursor::RestorePosition;
use crossterm::cursor::SavePosition;
use crossterm::style::Attribute;
use crossterm::style::Color;
use crossterm::style::Print;
use crossterm::style::ResetColor;
use crossterm::style::SetAttribute;
use crossterm::style::SetBackgroundColor;
use crossterm::style::SetForegroundColor;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneAgentState;
use muxr_core::PaneSnapshot;
use muxr_core::TabId;
use muxr_core::TabSnapshot;
use rootcause::prelude::ResultExt;

pub const WIDTH: u16 = 24;

const ACTIVE_FG: Color = Color::White;
const BACKGROUND: Color = Color::Rgb { r: 0, g: 19, b: 0 };
const INACTIVE_FG: Color = Color::Rgb { r: 119, g: 119, b: 119 };
const RAIL_ACTIVE_FG: Color = Color::Rgb { r: 106, g: 106, b: 223 };
const RAIL_INACTIVE_FG: Color = BACKGROUND;
const ROWS_PER_TAB: u16 = 2;
const SEPARATOR: &str = "\u{2502}";
const SEPARATOR_FG: Color = Color::Rgb { r: 50, g: 50, b: 50 };
const AGENT_BUSY_FG: Color = Color::Rgb { r: 140, g: 228, b: 121 };
const AGENT_UNSEEN_FG: Color = Color::Rgb { r: 255, g: 0, b: 0 };

#[derive(Clone, Debug, Eq, PartialEq)]
struct SidebarTab {
    active: bool,
    agent_state: PaneAgentState,
    cmd_label: Option<String>,
    path_label: String,
}

/// Queue the left tab sidebar.
///
/// # Errors
/// - The sidebar cmds cannot be written.
pub fn queue(stdout: &mut impl Write, layout: &LayoutSnapshot, rows: u16) -> rootcause::Result<()> {
    queue_cmd(stdout, SavePosition)?;

    let tabs = self::sidebar_tabs(layout);
    let mut row = 0;
    for tab in &tabs {
        if row >= rows {
            break;
        }
        self::queue_sidebar_row(stdout, row, tab.active, PaneAgentState::NoAgent, &tab.path_label)?;
        row = row.saturating_add(1);

        if row >= rows {
            break;
        }
        self::queue_sidebar_row(
            stdout,
            row,
            tab.active,
            tab.agent_state,
            tab.cmd_label.as_deref().unwrap_or(""),
        )?;
        row = row.saturating_add(1);
    }

    while row < rows {
        self::queue_sidebar_row(stdout, row, false, PaneAgentState::NoAgent, "")?;
        row = row.saturating_add(1);
    }

    queue_cmd(stdout, ResetColor)?;
    queue_cmd(stdout, SetAttribute(Attribute::Reset))?;
    queue_cmd(stdout, RestorePosition)?;
    Ok(())
}

#[must_use]
pub fn tab_id_at_row(layout: &LayoutSnapshot, row: u16) -> Option<TabId> {
    let index = usize::from(row / ROWS_PER_TAB);
    layout.tabs().get(index).map(|tab| tab.id().clone())
}

fn queue_sidebar_row(
    stdout: &mut impl Write,
    row: u16,
    active: bool,
    agent_state: PaneAgentState,
    text: &str,
) -> rootcause::Result<()> {
    let label_width = usize::from(WIDTH.saturating_sub(3));
    queue_cmd(stdout, MoveTo(0, row))?;
    queue_cmd(stdout, SetBackgroundColor(BACKGROUND))?;
    queue_cmd(
        stdout,
        SetForegroundColor(if active { RAIL_ACTIVE_FG } else { RAIL_INACTIVE_FG }),
    )?;
    queue_cmd(stdout, Print("\u{258e}"))?;
    queue_cmd(stdout, SetForegroundColor(if active { ACTIVE_FG } else { INACTIVE_FG }))?;
    if active {
        queue_cmd(stdout, SetAttribute(Attribute::Bold))?;
    }
    self::queue_agent_state_cell(stdout, active, agent_state)?;
    queue_cmd(stdout, Print(pad(text, label_width)))?;
    if active {
        queue_cmd(stdout, SetAttribute(Attribute::Reset))?;
        queue_cmd(stdout, SetBackgroundColor(BACKGROUND))?;
    }
    queue_cmd(stdout, SetForegroundColor(SEPARATOR_FG))?;
    queue_cmd(stdout, Print(SEPARATOR))?;
    Ok(())
}

fn queue_agent_state_cell(stdout: &mut impl Write, active: bool, agent_state: PaneAgentState) -> rootcause::Result<()> {
    let Some(color) = self::agent_state_dot_color(agent_state) else {
        queue_cmd(stdout, Print(" "))?;
        return Ok(());
    };

    queue_cmd(stdout, SetAttribute(Attribute::Bold))?;
    queue_cmd(stdout, SetForegroundColor(color))?;
    queue_cmd(stdout, Print("\u{2022}"))?;
    queue_cmd(stdout, SetAttribute(Attribute::Reset))?;
    queue_cmd(stdout, SetBackgroundColor(BACKGROUND))?;
    queue_cmd(stdout, SetForegroundColor(if active { ACTIVE_FG } else { INACTIVE_FG }))?;
    if active {
        queue_cmd(stdout, SetAttribute(Attribute::Bold))?;
    }
    Ok(())
}

const fn agent_state_dot_color(agent_state: PaneAgentState) -> Option<Color> {
    match agent_state {
        PaneAgentState::Busy => Some(AGENT_BUSY_FG),
        PaneAgentState::Unseen => Some(AGENT_UNSEEN_FG),
        PaneAgentState::NoAgent | PaneAgentState::Seen => None,
    }
}

fn sidebar_tabs(layout: &LayoutSnapshot) -> Vec<SidebarTab> {
    let home = std::env::var("HOME").ok();
    self::sidebar_tabs_with_home(layout, home.as_deref())
}

fn sidebar_tabs_with_home(layout: &LayoutSnapshot, home: Option<&str>) -> Vec<SidebarTab> {
    layout
        .tabs()
        .iter()
        .map(|tab| {
            let active = tab.id() == layout.active_tab();
            let display_pane = self::display_pane(tab, active);
            SidebarTab {
                active,
                agent_state: display_pane.map(|pane| pane.agent_state).unwrap_or_default(),
                cmd_label: self::cmd_label(display_pane),
                path_label: self::path_label(tab, display_pane, home),
            }
        })
        .collect()
}

fn display_pane(tab: &TabSnapshot, active: bool) -> Option<&PaneSnapshot> {
    if active && tab.panes().len() > 1 {
        return self::unfocused_unseen_agent_pane(tab).or_else(|| self::active_pane(tab));
    }

    self::highest_priority_agent_pane(tab).or_else(|| self::active_pane(tab))
}

fn unfocused_unseen_agent_pane(tab: &TabSnapshot) -> Option<&PaneSnapshot> {
    tab.panes()
        .iter()
        .find(|pane| pane.id.as_ref() != tab.active_pane().as_ref() && pane.agent_state == PaneAgentState::Unseen)
}

fn highest_priority_agent_pane(tab: &TabSnapshot) -> Option<&PaneSnapshot> {
    tab.panes()
        .iter()
        .filter(|pane| pane.agent_state != PaneAgentState::NoAgent)
        .max_by_key(|pane| pane.agent_state.priority())
}

fn cmd_label(pane: Option<&PaneSnapshot>) -> Option<String> {
    pane.and_then(|pane| pane.cmd_label.as_deref())
        .map(str::trim)
        .filter(|cmd| !cmd.is_empty())
        .map(ToOwned::to_owned)
}

fn path_label(tab: &TabSnapshot, pane: Option<&PaneSnapshot>, home: Option<&str>) -> String {
    let cwd = pane.map_or("", |pane| pane.cwd.trim());
    if cwd.is_empty() {
        tab.title().to_owned()
    } else {
        self::short_cwd_with_home(cwd, home)
    }
}

fn active_pane(tab: &TabSnapshot) -> Option<&PaneSnapshot> {
    tab.panes()
        .iter()
        .find(|pane| pane.id.as_ref() == tab.active_pane().as_ref())
}

fn short_cwd_with_home(cwd: &str, home: Option<&str>) -> String {
    let cwd = cwd.trim();
    if cwd.is_empty() {
        return String::new();
    }

    let (prefix, rest) = self::strip_home_or_root(cwd, home);
    let components = rest
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    if components.is_empty() {
        return prefix;
    }

    let mut out = prefix;
    for (index, component) in components.iter().enumerate() {
        let segment = if index.saturating_add(1) == components.len() {
            (*component).to_owned()
        } else {
            component.chars().next().map_or_else(String::new, |ch| ch.to_string())
        };
        self::push_path_segment(&mut out, &segment);
    }
    out
}

fn strip_home_or_root<'a>(cwd: &'a str, home: Option<&str>) -> (String, &'a str) {
    if let Some(home) = home.filter(|home| !home.is_empty())
        && (cwd == home || cwd.strip_prefix(home).is_some_and(|rest| rest.starts_with('/')))
    {
        let rest = cwd.strip_prefix(home).unwrap_or(cwd).trim_start_matches('/');
        return ("~".to_owned(), rest);
    }

    cwd.strip_prefix('/')
        .map_or_else(|| (String::new(), cwd), |rest| ("/".to_owned(), rest))
}

fn push_path_segment(out: &mut String, segment: &str) {
    if !out.is_empty() && out != "/" {
        out.push('/');
    }
    out.push_str(segment);
}

fn pad(text: &str, width: usize) -> String {
    let mut out = String::new();
    for ch in text.chars().take(width) {
        out.push(ch);
    }
    for _ in out.chars().count()..width {
        out.push(' ');
    }
    out
}

fn queue_cmd<W, C>(stdout: &mut W, cmd: C) -> rootcause::Result<()>
where
    W: Write,
    C: Command,
{
    Ok(stdout.queue(cmd).map(|_| ()).context("failed to write muxr tab bar")?)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_sidebar_tabs_when_second_tab_is_active_uses_active_pane_cwd() -> rootcause::Result<()> {
        let layout = muxr_core::LayoutSnapshot::new(
            muxr_core::TabId::new("tab-2")?,
            vec![
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new("tab-1")?,
                    "default",
                    muxr_core::PaneId::new("pane-1")?,
                    vec![muxr_core::PaneSnapshot {
                        agent_state: PaneAgentState::NoAgent,
                        cwd: "/Users/me/work/default".to_owned(),
                        cmd_label: None,
                        id: muxr_core::PaneId::new("pane-1")?,
                        title: "shell".to_owned(),
                    }],
                )?,
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new("tab-2")?,
                    "tab 2",
                    muxr_core::PaneId::new("pane-2")?,
                    vec![muxr_core::PaneSnapshot {
                        agent_state: PaneAgentState::NoAgent,
                        cwd: "/Users/me/src/muxr".to_owned(),
                        cmd_label: Some("nvim".to_owned()),
                        id: muxr_core::PaneId::new("pane-2")?,
                        title: "shell".to_owned(),
                    }],
                )?,
            ],
        )?;

        pretty_assertions::assert_eq!(
            sidebar_tabs_with_home(&layout, Some("/Users/me")),
            vec![
                SidebarTab {
                    active: false,
                    agent_state: PaneAgentState::NoAgent,
                    cmd_label: None,
                    path_label: "~/w/default".to_owned(),
                },
                SidebarTab {
                    active: true,
                    agent_state: PaneAgentState::NoAgent,
                    cmd_label: Some("nvim".to_owned()),
                    path_label: "~/s/muxr".to_owned(),
                },
            ],
        );
        Ok(())
    }

    #[test]
    fn test_sidebar_tabs_when_inactive_tab_has_agent_pane_uses_highest_priority_pane() -> rootcause::Result<()> {
        let tab_1 = muxr_core::TabId::new("tab-1")?;
        let tab_2 = muxr_core::TabId::new("tab-2")?;
        let layout = muxr_core::LayoutSnapshot::new(
            tab_1.clone(),
            vec![
                muxr_core::TabSnapshot::new(
                    tab_1,
                    "active",
                    muxr_core::PaneId::new("pane-1")?,
                    vec![muxr_core::PaneSnapshot {
                        agent_state: PaneAgentState::NoAgent,
                        cwd: "/tmp/active".to_owned(),
                        cmd_label: None,
                        id: muxr_core::PaneId::new("pane-1")?,
                        title: "shell".to_owned(),
                    }],
                )?,
                muxr_core::TabSnapshot::new(
                    tab_2,
                    "inactive",
                    muxr_core::PaneId::new("pane-2")?,
                    vec![
                        muxr_core::PaneSnapshot {
                            agent_state: PaneAgentState::NoAgent,
                            cwd: "/tmp/shell".to_owned(),
                            cmd_label: Some("zsh".to_owned()),
                            id: muxr_core::PaneId::new("pane-2")?,
                            title: "shell".to_owned(),
                        },
                        muxr_core::PaneSnapshot {
                            agent_state: PaneAgentState::Unseen,
                            cwd: "/tmp/codex".to_owned(),
                            cmd_label: Some("codex".to_owned()),
                            id: muxr_core::PaneId::new("pane-3")?,
                            title: "shell".to_owned(),
                        },
                    ],
                )?,
            ],
        )?;

        pretty_assertions::assert_eq!(
            sidebar_tabs_with_home(&layout, None),
            vec![
                SidebarTab {
                    active: true,
                    agent_state: PaneAgentState::NoAgent,
                    cmd_label: None,
                    path_label: "/t/active".to_owned(),
                },
                SidebarTab {
                    active: false,
                    agent_state: PaneAgentState::Unseen,
                    cmd_label: Some("codex".to_owned()),
                    path_label: "/t/codex".to_owned(),
                },
            ],
        );
        Ok(())
    }

    #[test]
    fn test_sidebar_tabs_when_active_tab_has_unfocused_unseen_agent_uses_agent_pane() -> rootcause::Result<()> {
        let active_tab = muxr_core::TabId::new("tab-1")?;
        let active_pane = muxr_core::PaneId::new("pane-1")?;
        let layout = muxr_core::LayoutSnapshot::new(
            active_tab.clone(),
            vec![muxr_core::TabSnapshot::new(
                active_tab,
                "default",
                active_pane,
                vec![
                    muxr_core::PaneSnapshot {
                        agent_state: PaneAgentState::NoAgent,
                        cwd: "/tmp/shell".to_owned(),
                        cmd_label: Some("zsh".to_owned()),
                        id: muxr_core::PaneId::new("pane-1")?,
                        title: "shell".to_owned(),
                    },
                    muxr_core::PaneSnapshot {
                        agent_state: PaneAgentState::Unseen,
                        cwd: "/tmp/codex".to_owned(),
                        cmd_label: Some("codex".to_owned()),
                        id: muxr_core::PaneId::new("pane-2")?,
                        title: "shell".to_owned(),
                    },
                ],
            )?],
        )?;

        pretty_assertions::assert_eq!(
            sidebar_tabs_with_home(&layout, None),
            vec![SidebarTab {
                active: true,
                agent_state: PaneAgentState::Unseen,
                cmd_label: Some("codex".to_owned()),
                path_label: "/t/codex".to_owned(),
            }],
        );
        Ok(())
    }

    #[rstest]
    #[case::home_project("/Users/me/project", Some("/Users/me"), "~/project")]
    #[case::home_nested("/Users/me/src/pkg/project", Some("/Users/me"), "~/s/p/project")]
    #[case::root_nested("/usr/local/bin", Some("/Users/me"), "/u/l/bin")]
    #[case::relative("target/debug", Some("/Users/me"), "t/debug")]
    fn test_short_cwd_with_home_returns_compact_path(
        #[case] cwd: &str,
        #[case] home: Option<&str>,
        #[case] expected: &str,
    ) {
        pretty_assertions::assert_eq!(short_cwd_with_home(cwd, home), expected);
    }

    #[test]
    fn test_queue_when_layout_is_rendered_writes_sidebar_without_flushing() -> rootcause::Result<()> {
        let active_tab = muxr_core::TabId::new("tab-1")?;
        let active_pane = muxr_core::PaneId::new("pane-1")?;
        let pane = muxr_core::PaneSnapshot {
            agent_state: PaneAgentState::Busy,
            cwd: "project".to_owned(),
            cmd_label: Some("codex".to_owned()),
            id: active_pane.clone(),
            title: "shell".to_owned(),
        };
        let tab = muxr_core::TabSnapshot::new(active_tab.clone(), "default", active_pane, vec![pane])?;
        let layout = muxr_core::LayoutSnapshot::new(active_tab, vec![tab])?;
        let mut output = CountingWriter::default();

        queue(&mut output, &layout, 3)?;

        let rendered = output.rendered_string()?;
        assert2::assert!(rendered.contains("project"));
        assert2::assert!(rendered.contains("codex"));
        assert2::assert!(rendered.contains("\u{2022}"));
        assert2::assert!(rendered.contains(SEPARATOR));
        pretty_assertions::assert_eq!(output.flushes, 0);
        Ok(())
    }

    #[rstest]
    #[case::first_label_row(0, Some("tab-1"))]
    #[case::first_blank_row(1, Some("tab-1"))]
    #[case::second_label_row(2, Some("tab-2"))]
    #[case::below_tabs(4, None)]
    fn test_tab_id_at_row_when_row_varies_returns_clicked_tab(
        #[case] row: u16,
        #[case] expected: Option<&str>,
    ) -> rootcause::Result<()> {
        let layout = muxr_core::LayoutSnapshot::new(
            muxr_core::TabId::new("tab-1")?,
            vec![
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new("tab-1")?,
                    "default",
                    muxr_core::PaneId::new("pane-1")?,
                    vec![muxr_core::PaneSnapshot {
                        agent_state: PaneAgentState::NoAgent,
                        cwd: "default".to_owned(),
                        cmd_label: None,
                        id: muxr_core::PaneId::new("pane-1")?,
                        title: "shell".to_owned(),
                    }],
                )?,
                muxr_core::TabSnapshot::new(
                    muxr_core::TabId::new("tab-2")?,
                    "tab 2",
                    muxr_core::PaneId::new("pane-2")?,
                    vec![muxr_core::PaneSnapshot {
                        agent_state: PaneAgentState::NoAgent,
                        cwd: "tab-2".to_owned(),
                        cmd_label: None,
                        id: muxr_core::PaneId::new("pane-2")?,
                        title: "shell".to_owned(),
                    }],
                )?,
            ],
        )?;

        pretty_assertions::assert_eq!(tab_id_at_row(&layout, row).as_ref().map(TabId::as_ref), expected,);
        Ok(())
    }

    #[derive(Default)]
    struct CountingWriter {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl CountingWriter {
        fn rendered_string(&self) -> rootcause::Result<String> {
            Ok(String::from_utf8(self.bytes.clone()).context("muxr tab bar test output was not utf8")?)
        }
    }

    impl std::io::Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes = self.flushes.saturating_add(1);
            Ok(())
        }
    }
}
