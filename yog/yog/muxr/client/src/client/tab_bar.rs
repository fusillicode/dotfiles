use std::io::Write;

use crossterm::Command;
use crossterm::QueueableCommand;
use crossterm::cursor::MoveTo;
use crossterm::cursor::RestorePosition;
use crossterm::cursor::SavePosition;
use crossterm::style::Attribute;
use crossterm::style::Print;
use crossterm::style::ResetColor;
use crossterm::style::SetAttribute;
use crossterm::style::SetBackgroundColor;
use crossterm::style::SetForegroundColor;
use muxr_config::TabBarConfig;
use muxr_core::LayoutSnapshot;
use muxr_core::PaneSnapshot;
use muxr_core::RenderColor;
use muxr_core::TabId;
use muxr_core::TabSnapshot;
use muxr_core::TrackedProcessState;
use rootcause::prelude::ResultExt;

const ROWS_PER_TAB: u16 = 3;
const SEPARATOR: &str = "\u{2502}";

#[derive(Clone, Debug, Eq, PartialEq)]
struct SidebarTab {
    active: bool,
    tracked_process_state: TrackedProcessState,
    cmd_label: Option<String>,
    path_label: String,
}

/// Queue the left tab sidebar.
///
/// # Errors
/// - The sidebar cmds cannot be written.
pub fn queue(
    stdout: &mut impl Write,
    config: TabBarConfig,
    layout: &LayoutSnapshot,
    rows: u16,
) -> rootcause::Result<()> {
    queue_cmd(stdout, SavePosition)?;

    let tabs = self::sidebar_tabs(layout);
    let mut row = 0;
    for tab in &tabs {
        if row >= rows {
            break;
        }
        self::queue_sidebar_row(
            stdout,
            config,
            row,
            tab.active,
            TrackedProcessState::None,
            &tab.path_label,
        )?;
        row = row.saturating_add(1);

        if row >= rows {
            break;
        }
        self::queue_sidebar_row(
            stdout,
            config,
            row,
            tab.active,
            tab.tracked_process_state,
            tab.cmd_label.as_deref().unwrap_or(""),
        )?;
        row = row.saturating_add(1);

        if row >= rows {
            break;
        }
        // Keep each tab entry a stable three-row block with a spacer row.
        self::queue_sidebar_row(stdout, config, row, tab.active, TrackedProcessState::None, "")?;
        row = row.saturating_add(1);
    }

    while row < rows {
        self::queue_sidebar_row(stdout, config, row, false, TrackedProcessState::None, "")?;
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
    config: TabBarConfig,
    row: u16,
    active: bool,
    tracked_process_state: TrackedProcessState,
    text: &str,
) -> rootcause::Result<()> {
    let content_width = usize::from(config.width.saturating_sub(2));
    queue_cmd(stdout, MoveTo(0, row))?;
    queue_cmd(stdout, SetBackgroundColor(crate::render::crossterm_color(config.bg)))?;
    queue_cmd(
        stdout,
        SetForegroundColor(if active {
            crate::render::crossterm_color(config.rail.active_fg)
        } else {
            crate::render::crossterm_color(config.rail.inactive_fg)
        }),
    )?;
    queue_cmd(stdout, Print("\u{258e}"))?;
    self::queue_sidebar_text_style(stdout, config, active)?;
    // Keep normal labels flush after the rail; marker rows prefix the dot and one space.
    let marker_width = if self::tracked_process_state_dot_color(config, tracked_process_state).is_some() {
        2
    } else {
        0
    };
    let label = text
        .chars()
        .take(content_width.saturating_sub(marker_width))
        .collect::<String>();
    let used_width = label.chars().count().saturating_add(marker_width);
    self::queue_tracked_process_state_marker(stdout, config, active, tracked_process_state)?;
    queue_cmd(stdout, Print(&label))?;
    self::queue_sidebar_text_style(stdout, config, active)?;
    let trailing_width = content_width.saturating_sub(used_width);
    if trailing_width > 0 {
        queue_cmd(stdout, Print(pad("", trailing_width)))?;
    }
    queue_cmd(stdout, SetAttribute(Attribute::Reset))?;
    queue_cmd(stdout, SetBackgroundColor(crate::render::crossterm_color(config.bg)))?;
    queue_cmd(
        stdout,
        SetForegroundColor(crate::render::crossterm_color(config.separator_fg)),
    )?;
    queue_cmd(stdout, Print(SEPARATOR))?;
    Ok(())
}

fn queue_sidebar_text_style(stdout: &mut impl Write, config: TabBarConfig, active: bool) -> rootcause::Result<()> {
    queue_cmd(stdout, SetAttribute(Attribute::Reset))?;
    queue_cmd(stdout, SetBackgroundColor(crate::render::crossterm_color(config.bg)))?;
    queue_cmd(
        stdout,
        SetForegroundColor(if active {
            crate::render::crossterm_color(config.active_fg)
        } else {
            crate::render::crossterm_color(config.inactive_fg)
        }),
    )?;
    if active {
        queue_cmd(stdout, SetAttribute(Attribute::Bold))?;
    }
    Ok(())
}

fn queue_tracked_process_state_marker(
    stdout: &mut impl Write,
    config: TabBarConfig,
    active: bool,
    tracked_process_state: TrackedProcessState,
) -> rootcause::Result<()> {
    let Some(color) = self::tracked_process_state_dot_color(config, tracked_process_state) else {
        return Ok(());
    };

    queue_cmd(stdout, SetAttribute(Attribute::Bold))?;
    queue_cmd(stdout, SetForegroundColor(crate::render::crossterm_color(color)))?;
    queue_cmd(stdout, Print("\u{2022}"))?;
    self::queue_sidebar_text_style(stdout, config, active)?;
    queue_cmd(stdout, Print(" "))?;
    Ok(())
}

const fn tracked_process_state_dot_color(
    config: TabBarConfig,
    tracked_process_state: TrackedProcessState,
) -> Option<RenderColor> {
    match tracked_process_state {
        TrackedProcessState::Busy => Some(config.tracked_process.busy_fg),
        TrackedProcessState::Unseen => Some(config.tracked_process.unseen_fg),
        TrackedProcessState::None | TrackedProcessState::Seen => None,
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
                tracked_process_state: display_pane.map(|pane| pane.tracked_process_state).unwrap_or_default(),
                cmd_label: self::cmd_label(display_pane),
                path_label: self::path_label(tab, display_pane, home),
            }
        })
        .collect()
}

fn display_pane(tab: &TabSnapshot, active: bool) -> Option<&PaneSnapshot> {
    if active && tab.panes().len() > 1 {
        return self::unfocused_unseen_tracked_process_pane(tab).or_else(|| self::active_pane(tab));
    }

    self::inactive_tab_display_pane(tab)
}

fn unfocused_unseen_tracked_process_pane(tab: &TabSnapshot) -> Option<&PaneSnapshot> {
    tab.panes()
        .iter()
        .find(|pane| &pane.id != tab.active_pane() && pane.tracked_process_state == TrackedProcessState::Unseen)
}

fn inactive_tab_display_pane(tab: &TabSnapshot) -> Option<&PaneSnapshot> {
    // Inactive tabs need one representative pane: attention/running state wins, while
    // idle tracked processes still keep their label and no dot. Ties use focus recency.
    self::focused_pane_with_tracked_process_state(tab, TrackedProcessState::Unseen)
        .or_else(|| self::focused_pane_with_tracked_process_state(tab, TrackedProcessState::Busy))
        .or_else(|| self::focused_pane_with_tracked_process_state(tab, TrackedProcessState::Seen))
        .or_else(|| self::active_pane(tab))
}

fn focused_pane_with_tracked_process_state(
    tab: &TabSnapshot,
    tracked_process_state: TrackedProcessState,
) -> Option<&PaneSnapshot> {
    tab.panes()
        .iter()
        .filter(|pane| pane.tracked_process_state == tracked_process_state)
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
    use muxr_config::MuxrConfig;
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
                        TrackedProcessState::None,
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
                        TrackedProcessState::None,
                    )?],
                )?,
            ],
        )?;

        pretty_assertions::assert_eq!(
            sidebar_tabs_with_home(&layout, Some("/Users/me")),
            vec![
                SidebarTab {
                    active: false,
                    tracked_process_state: TrackedProcessState::None,
                    cmd_label: None,
                    path_label: "~/w/default".to_owned(),
                },
                SidebarTab {
                    active: true,
                    tracked_process_state: TrackedProcessState::None,
                    cmd_label: Some("nvim".to_owned()),
                    path_label: "~/s/muxr".to_owned(),
                },
            ],
        );
        Ok(())
    }

    #[test]
    fn test_sidebar_tabs_when_inactive_tab_has_unseen_tracked_process_pane_uses_unseen_pane() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![
                self::tab_snapshot(
                    1,
                    "active",
                    1,
                    vec![self::pane_snapshot(1, "/tmp/active", None, TrackedProcessState::None)?],
                )?,
                self::tab_snapshot(
                    2,
                    "inactive",
                    2,
                    vec![
                        self::pane_snapshot(2, "/tmp/shell", Some("zsh"), TrackedProcessState::None)?,
                        self::pane_snapshot(4, "/tmp/cargo", Some("cargo test"), TrackedProcessState::Busy)?,
                        self::pane_snapshot(3, "/tmp/codex", Some("codex"), TrackedProcessState::Unseen)?,
                    ],
                )?,
            ],
        )?;

        pretty_assertions::assert_eq!(
            sidebar_tabs_with_home(&layout, None),
            vec![
                SidebarTab {
                    active: true,
                    tracked_process_state: TrackedProcessState::None,
                    cmd_label: None,
                    path_label: "/t/active".to_owned(),
                },
                SidebarTab {
                    active: false,
                    tracked_process_state: TrackedProcessState::Unseen,
                    cmd_label: Some("codex".to_owned()),
                    path_label: "/t/codex".to_owned(),
                },
            ],
        );
        Ok(())
    }

    #[rstest]
    #[case::unseen(
        TrackedProcessState::Unseen,
        "/tmp/unseen-old",
        "codex-old",
        "/tmp/unseen-recent",
        "codex-recent",
        "/t/unseen-recent",
        "codex-recent"
    )]
    #[case::busy(
        TrackedProcessState::Busy,
        "/tmp/busy-old",
        "claude-old",
        "/tmp/busy-recent",
        "claude-recent",
        "/t/busy-recent",
        "claude-recent"
    )]
    #[case::seen(
        TrackedProcessState::Seen,
        "/tmp/seen-old",
        "cursor-old",
        "/tmp/seen-recent",
        "cursor-recent",
        "/t/seen-recent",
        "cursor-recent"
    )]
    fn test_sidebar_tabs_when_inactive_tab_has_multiple_tracked_processes_in_same_state_uses_last_focused(
        #[case] tracked_process_state: TrackedProcessState,
        #[case] first_cwd: &str,
        #[case] first_cmd_label: &str,
        #[case] second_cwd: &str,
        #[case] second_cmd_label: &str,
        #[case] expected_path_label: &str,
        #[case] expected_cmd_label: &str,
    ) -> rootcause::Result<()> {
        let mut older_tracked_process_pane =
            self::pane_snapshot(4, first_cwd, Some(first_cmd_label), tracked_process_state)?;
        older_tracked_process_pane.focus_seq = 10;
        let mut recent_tracked_process_pane =
            self::pane_snapshot(3, second_cwd, Some(second_cmd_label), tracked_process_state)?;
        recent_tracked_process_pane.focus_seq = 20;

        let layout = self::layout_snapshot(
            1,
            vec![
                self::tab_snapshot(
                    1,
                    "active",
                    1,
                    vec![self::pane_snapshot(1, "/tmp/active", None, TrackedProcessState::None)?],
                )?,
                self::tab_snapshot(
                    2,
                    "inactive",
                    2,
                    vec![
                        self::pane_snapshot(2, "/tmp/shell", Some("zsh"), TrackedProcessState::None)?,
                        older_tracked_process_pane,
                        recent_tracked_process_pane,
                    ],
                )?,
            ],
        )?;

        let tabs = sidebar_tabs_with_home(&layout, None);

        pretty_assertions::assert_eq!(
            tabs[1],
            SidebarTab {
                active: false,
                tracked_process_state,
                cmd_label: Some(expected_cmd_label.to_owned()),
                path_label: expected_path_label.to_owned(),
            },
        );
        Ok(())
    }

    #[test]
    fn test_sidebar_tabs_when_inactive_tab_has_seen_tracked_process_uses_tracked_process_pane_without_dot_state()
    -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![
                self::tab_snapshot(
                    1,
                    "active",
                    1,
                    vec![self::pane_snapshot(1, "/tmp/active", None, TrackedProcessState::None)?],
                )?,
                self::tab_snapshot(
                    2,
                    "inactive",
                    2,
                    vec![
                        self::pane_snapshot(2, "/tmp/shell", Some("zsh"), TrackedProcessState::None)?,
                        self::pane_snapshot(3, "/tmp/codex", Some("codex"), TrackedProcessState::Seen)?,
                    ],
                )?,
            ],
        )?;

        let tabs = sidebar_tabs_with_home(&layout, None);

        pretty_assertions::assert_eq!(
            tabs[1],
            SidebarTab {
                active: false,
                tracked_process_state: TrackedProcessState::Seen,
                cmd_label: Some("codex".to_owned()),
                path_label: "/t/codex".to_owned(),
            },
        );
        Ok(())
    }

    #[test]
    fn test_sidebar_tabs_when_active_tab_has_unfocused_unseen_tracked_process_uses_tracked_process_pane()
    -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![
                    self::pane_snapshot(1, "/tmp/shell", Some("zsh"), TrackedProcessState::None)?,
                    self::pane_snapshot(2, "/tmp/codex", Some("codex"), TrackedProcessState::Unseen)?,
                ],
            )?],
        )?;

        pretty_assertions::assert_eq!(
            sidebar_tabs_with_home(&layout, None),
            vec![SidebarTab {
                active: true,
                tracked_process_state: TrackedProcessState::Unseen,
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
                vec![self::pane_snapshot(
                    1,
                    "project",
                    Some("codex"),
                    TrackedProcessState::Busy,
                )?],
            )?],
        )?;
        let mut output = CountingWriter::default();

        queue(&mut output, MuxrConfig::default().tab_bar, &layout, 3)?;

        let rendered = output.rendered_string()?;
        assert2::assert!(rendered.contains("project"));
        assert2::assert!(rendered.contains("codex"));
        assert2::assert!(rendered.contains("\u{2022}"));
        assert2::assert!(rendered.contains(SEPARATOR));
        pretty_assertions::assert_eq!(output.flushes, 0);
        Ok(())
    }

    #[test]
    fn test_queue_when_seen_tracked_process_is_rendered_shows_label_without_marker() -> rootcause::Result<()> {
        let layout = self::layout_snapshot(
            1,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![self::pane_snapshot(
                    1,
                    "project",
                    Some("cx"),
                    TrackedProcessState::Seen,
                )?],
            )?],
        )?;
        let mut output = CountingWriter::default();

        queue(&mut output, MuxrConfig::default().tab_bar, &layout, 2)?;

        let visible = self::strip_ansi(&output.rendered_string()?);
        assert2::assert!(visible.contains("\u{258e}cx"));
        assert2::assert!(!visible.contains("\u{2022} cx"));
        Ok(())
    }

    #[test]
    fn test_queue_when_tracked_process_marker_is_rendered_keeps_labels_flush_and_spaces_marker() -> rootcause::Result<()>
    {
        let layout = self::layout_snapshot(
            1,
            vec![self::tab_snapshot(
                1,
                "default",
                1,
                vec![self::pane_snapshot(
                    1,
                    "project",
                    Some("cx"),
                    TrackedProcessState::Busy,
                )?],
            )?],
        )?;
        let mut output = CountingWriter::default();

        queue(&mut output, MuxrConfig::default().tab_bar, &layout, 2)?;

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
                vec![self::pane_snapshot(
                    1,
                    "project",
                    Some("cx"),
                    TrackedProcessState::Busy,
                )?],
            )?],
        )?;
        let mut output = CountingWriter::default();

        queue(&mut output, MuxrConfig::default().tab_bar, &layout, 3)?;

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
                    vec![self::pane_snapshot(1, "default", None, TrackedProcessState::None)?],
                )?,
                self::tab_snapshot(
                    2,
                    "tab 2",
                    2,
                    vec![self::pane_snapshot(2, "tab-2", None, TrackedProcessState::None)?],
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
        tracked_process_state: TrackedProcessState,
    ) -> rootcause::Result<PaneSnapshot> {
        Ok(PaneSnapshot {
            tracked_process_state,
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
