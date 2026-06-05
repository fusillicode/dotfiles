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
const ROWS_PER_TAB: u16 = 3;
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

        if row >= rows {
            break;
        }
        // Keep muxr tab entries aligned with the three-row agg tab-bar shape.
        self::queue_sidebar_row(stdout, row, tab.active, PaneAgentState::NoAgent, "")?;
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
    layout.tabs().get(index).map(|tab| *tab.id())
}

fn queue_sidebar_row(
    stdout: &mut impl Write,
    row: u16,
    active: bool,
    agent_state: PaneAgentState,
    text: &str,
) -> rootcause::Result<()> {
    let content_width = usize::from(WIDTH.saturating_sub(2));
    queue_cmd(stdout, MoveTo(0, row))?;
    queue_cmd(stdout, SetBackgroundColor(BACKGROUND))?;
    queue_cmd(
        stdout,
        SetForegroundColor(if active { RAIL_ACTIVE_FG } else { RAIL_INACTIVE_FG }),
    )?;
    queue_cmd(stdout, Print("\u{258e}"))?;
    self::queue_sidebar_text_style(stdout, active)?;
    // Keep normal labels flush after the rail; marker rows prefix the dot and one space.
    let marker_width = if self::agent_state_dot_color(agent_state).is_some() {
        2
    } else {
        0
    };
    let label = text
        .chars()
        .take(content_width.saturating_sub(marker_width))
        .collect::<String>();
    let used_width = label.chars().count().saturating_add(marker_width);
    self::queue_agent_state_marker(stdout, active, agent_state)?;
    queue_cmd(stdout, Print(&label))?;
    self::queue_sidebar_text_style(stdout, active)?;
    let trailing_width = content_width.saturating_sub(used_width);
    if trailing_width > 0 {
        queue_cmd(stdout, Print(pad("", trailing_width)))?;
    }
    queue_cmd(stdout, SetAttribute(Attribute::Reset))?;
    queue_cmd(stdout, SetBackgroundColor(BACKGROUND))?;
    queue_cmd(stdout, SetForegroundColor(SEPARATOR_FG))?;
    queue_cmd(stdout, Print(SEPARATOR))?;
    Ok(())
}

fn queue_sidebar_text_style(stdout: &mut impl Write, active: bool) -> rootcause::Result<()> {
    queue_cmd(stdout, SetAttribute(Attribute::Reset))?;
    queue_cmd(stdout, SetBackgroundColor(BACKGROUND))?;
    queue_cmd(stdout, SetForegroundColor(if active { ACTIVE_FG } else { INACTIVE_FG }))?;
    if active {
        queue_cmd(stdout, SetAttribute(Attribute::Bold))?;
    }
    Ok(())
}

fn queue_agent_state_marker(
    stdout: &mut impl Write,
    active: bool,
    agent_state: PaneAgentState,
) -> rootcause::Result<()> {
    let Some(color) = self::agent_state_dot_color(agent_state) else {
        return Ok(());
    };

    queue_cmd(stdout, SetAttribute(Attribute::Bold))?;
    queue_cmd(stdout, SetForegroundColor(color))?;
    queue_cmd(stdout, Print("\u{2022}"))?;
    self::queue_sidebar_text_style(stdout, active)?;
    queue_cmd(stdout, Print(" "))?;
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

    self::inactive_tab_display_pane(tab)
}

fn unfocused_unseen_agent_pane(tab: &TabSnapshot) -> Option<&PaneSnapshot> {
    tab.panes()
        .iter()
        .find(|pane| &pane.id != tab.active_pane() && pane.agent_state == PaneAgentState::Unseen)
}

fn inactive_tab_display_pane(tab: &TabSnapshot) -> Option<&PaneSnapshot> {
    // Inactive tabs need one representative pane: attention/running state wins, while
    // idle agents still keep their label and no dot. Ties use focus recency.
    self::focused_pane_with_agent_state(tab, PaneAgentState::Unseen)
        .or_else(|| self::focused_pane_with_agent_state(tab, PaneAgentState::Busy))
        .or_else(|| self::focused_pane_with_agent_state(tab, PaneAgentState::Seen))
        .or_else(|| self::active_pane(tab))
}

fn focused_pane_with_agent_state(tab: &TabSnapshot, agent_state: PaneAgentState) -> Option<&PaneSnapshot> {
    tab.panes()
        .iter()
        .filter(|pane| pane.agent_state == agent_state)
        .max_by_key(|pane| pane.focus_seq)
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
    tab.panes().iter().find(|pane| &pane.id == tab.active_pane())
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
    use muxr_core::PaneId;
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_sidebar_tabs_when_second_tab_is_active_uses_active_pane_cwd() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            2,
            vec![
                self::tab_snapshot(
                    1,
                    "default",
                    1,
                    vec![self::pane_snapshot(
                        1,
                        "/Users/me/work/default",
                        None,
                        PaneAgentState::NoAgent,
                    )?],
                )?,
                self::tab_snapshot(
                    2,
                    "tab 2",
                    2,
                    vec![self::pane_snapshot(
                        2,
                        "/Users/me/src/muxr",
                        Some("nvim"),
                        PaneAgentState::NoAgent,
                    )?],
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
    fn test_sidebar_tabs_when_inactive_tab_has_unseen_agent_pane_uses_unseen_pane() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![
                self::tab_snapshot(
                    1,
                    "active",
                    1,
                    vec![self::pane_snapshot(1, "/tmp/active", None, PaneAgentState::NoAgent)?],
                )?,
                self::tab_snapshot(
                    2,
                    "inactive",
                    2,
                    vec![
                        self::pane_snapshot(2, "/tmp/shell", Some("zsh"), PaneAgentState::NoAgent)?,
                        self::pane_snapshot(4, "/tmp/cargo", Some("cargo test"), PaneAgentState::Busy)?,
                        self::pane_snapshot(3, "/tmp/codex", Some("codex"), PaneAgentState::Unseen)?,
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

    #[rstest]
    #[case::unseen(
        PaneAgentState::Unseen,
        "/tmp/unseen-old",
        "codex-old",
        "/tmp/unseen-recent",
        "codex-recent",
        "/t/unseen-recent",
        "codex-recent"
    )]
    #[case::busy(
        PaneAgentState::Busy,
        "/tmp/busy-old",
        "claude-old",
        "/tmp/busy-recent",
        "claude-recent",
        "/t/busy-recent",
        "claude-recent"
    )]
    #[case::seen(
        PaneAgentState::Seen,
        "/tmp/seen-old",
        "cursor-old",
        "/tmp/seen-recent",
        "cursor-recent",
        "/t/seen-recent",
        "cursor-recent"
    )]
    fn test_sidebar_tabs_when_inactive_tab_has_multiple_agents_in_same_state_uses_last_focused(
        #[case] agent_state: PaneAgentState,
        #[case] first_cwd: &str,
        #[case] first_cmd_label: &str,
        #[case] second_cwd: &str,
        #[case] second_cmd_label: &str,
        #[case] expected_path_label: &str,
        #[case] expected_cmd_label: &str,
    ) -> rootcause::Result<()> {
        let mut older_agent_pane = self::pane_snapshot(4, first_cwd, Some(first_cmd_label), agent_state)?;
        older_agent_pane.focus_seq = 10;
        let mut recent_agent_pane = self::pane_snapshot(3, second_cwd, Some(second_cmd_label), agent_state)?;
        recent_agent_pane.focus_seq = 20;

        let layout = self::layout_snapshot(
            1,
            vec![
                self::tab_snapshot(
                    1,
                    "active",
                    1,
                    vec![self::pane_snapshot(1, "/tmp/active", None, PaneAgentState::NoAgent)?],
                )?,
                self::tab_snapshot(
                    2,
                    "inactive",
                    2,
                    vec![
                        self::pane_snapshot(2, "/tmp/shell", Some("zsh"), PaneAgentState::NoAgent)?,
                        older_agent_pane,
                        recent_agent_pane,
                    ],
                )?,
            ],
        )?;

        let tabs = sidebar_tabs_with_home(&layout, None);

        pretty_assertions::assert_eq!(
            tabs[1],
            SidebarTab {
                active: false,
                agent_state,
                cmd_label: Some(expected_cmd_label.to_owned()),
                path_label: expected_path_label.to_owned(),
            },
        );
        Ok(())
    }

    #[test]
    fn test_sidebar_tabs_when_inactive_tab_has_seen_agent_uses_agent_pane_without_dot_state() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![
                self::tab_snapshot(
                    1,
                    "active",
                    1,
                    vec![self::pane_snapshot(1, "/tmp/active", None, PaneAgentState::NoAgent)?],
                )?,
                self::tab_snapshot(
                    2,
                    "inactive",
                    2,
                    vec![
                        self::pane_snapshot(2, "/tmp/shell", Some("zsh"), PaneAgentState::NoAgent)?,
                        self::pane_snapshot(3, "/tmp/codex", Some("codex"), PaneAgentState::Seen)?,
                    ],
                )?,
            ],
        )?;

        let tabs = sidebar_tabs_with_home(&layout, None);

        pretty_assertions::assert_eq!(
            tabs[1],
            SidebarTab {
                active: false,
                agent_state: PaneAgentState::Seen,
                cmd_label: Some("codex".to_owned()),
                path_label: "/t/codex".to_owned(),
            },
        );
        Ok(())
    }

    #[test]
    fn test_sidebar_tabs_when_active_tab_has_unfocused_unseen_agent_uses_agent_pane() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![
                    self::pane_snapshot(1, "/tmp/shell", Some("zsh"), PaneAgentState::NoAgent)?,
                    self::pane_snapshot(2, "/tmp/codex", Some("codex"), PaneAgentState::Unseen)?,
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
        let layout = self::layout_snapshot(
            1,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![self::pane_snapshot(1, "project", Some("codex"), PaneAgentState::Busy)?],
            )?],
        )?;
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

    #[test]
    fn test_queue_when_seen_agent_is_rendered_shows_label_without_marker() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![self::pane_snapshot(1, "project", Some("cx"), PaneAgentState::Seen)?],
            )?],
        )?;
        let mut output = CountingWriter::default();

        queue(&mut output, &layout, 2)?;

        let visible = self::strip_ansi(&output.rendered_string()?);
        assert2::assert!(visible.contains("\u{258e}cx"));
        assert2::assert!(!visible.contains("\u{2022} cx"));
        Ok(())
    }

    #[test]
    fn test_queue_when_agent_marker_is_rendered_keeps_labels_flush_and_spaces_marker() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![self::pane_snapshot(1, "project", Some("cx"), PaneAgentState::Busy)?],
            )?],
        )?;
        let mut output = CountingWriter::default();

        queue(&mut output, &layout, 2)?;

        let visible = self::strip_ansi(&output.rendered_string()?);
        assert2::assert!(visible.contains("\u{258e}project"));
        assert2::assert!(visible.contains("\u{258e}\u{2022} cx"));
        assert2::assert!(!visible.contains("\u{258e} project"));
        assert2::assert!(!visible.contains("\u{258e} cx"));
        assert2::assert!(!visible.contains("\u{258e}cx \u{2022}"));
        Ok(())
    }

    #[test]
    fn test_queue_when_tab_is_rendered_adds_spacer_row_after_cmd_row() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![self::pane_snapshot(1, "project", Some("cx"), PaneAgentState::Busy)?],
            )?],
        )?;
        let mut output = CountingWriter::default();

        queue(&mut output, &layout, 3)?;

        let visible = self::strip_ansi(&output.rendered_string()?);
        let rows = visible.split(SEPARATOR).collect::<Vec<_>>();
        pretty_assertions::assert_eq!(rows.len(), 4);
        assert2::assert!(rows[0].starts_with("\u{258e}project"));
        assert2::assert!(rows[1].starts_with("\u{258e}\u{2022} cx"));
        pretty_assertions::assert_eq!(rows[2].trim(), "\u{258e}");
        Ok(())
    }

    #[rstest]
    #[case::first_path_row(0, Some(1))]
    #[case::first_cmd_row(1, Some(1))]
    #[case::first_spacer_row(2, Some(1))]
    #[case::second_path_row(3, Some(2))]
    #[case::second_cmd_row(4, Some(2))]
    #[case::second_spacer_row(5, Some(2))]
    #[case::below_tabs(6, None)]
    fn test_tab_id_at_row_when_row_varies_returns_clicked_tab(
        #[case] row: u16,
        #[case] expected: Option<u32>,
    ) -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![
                self::tab_snapshot(
                    1,
                    "default",
                    1,
                    vec![self::pane_snapshot(1, "default", None, PaneAgentState::NoAgent)?],
                )?,
                self::tab_snapshot(
                    2,
                    "tab 2",
                    2,
                    vec![self::pane_snapshot(2, "tab-2", None, PaneAgentState::NoAgent)?],
                )?,
            ],
        )?;

        pretty_assertions::assert_eq!(tab_id_at_row(&layout, row).map(TabId::get), expected);
        Ok(())
    }

    fn layout_snapshot(active_tab: u32, tabs: Vec<TabSnapshot>) -> rootcause::Result<LayoutSnapshot> {
        LayoutSnapshot::new(TabId::new(active_tab)?, tabs)
    }

    fn tab_snapshot(
        id: u32,
        title: &str,
        active_pane: u32,
        panes: Vec<PaneSnapshot>,
    ) -> rootcause::Result<TabSnapshot> {
        TabSnapshot::new(TabId::new(id)?, title, PaneId::new(active_pane)?, panes)
    }

    fn pane_snapshot(
        id: u32,
        cwd: &str,
        cmd_label: Option<&str>,
        agent_state: PaneAgentState,
    ) -> rootcause::Result<PaneSnapshot> {
        Ok(PaneSnapshot {
            agent_state,
            cwd: cwd.to_owned(),
            cmd_label: cmd_label.map(str::to_owned),
            focus_seq: u64::from(id),
            id: PaneId::new(id)?,
            title: "shell".to_owned(),
        })
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

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch != '\x1b' {
                out.push(ch);
                continue;
            }
            for escaped in chars.by_ref() {
                if escaped.is_ascii_alphabetic() {
                    break;
                }
            }
        }
        out
    }
}
