use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use zellij_tile::prelude::*;

const FILE_LOCATION_REGEX: &str = r"\S+";
const DEFAULT_COLUMN: usize = 1;
const DEFAULT_LINE: usize = 1;
const EXISTS_CHECK_KIND: &str = "zop-exists-check";
const CONTEXT_KIND: &str = "kind";
const CONTEXT_REQUEST_ID: &str = "request_id";
const RIGHT_PANE_RESIZE_LEFT_COUNT: usize = 2;
const NVIM_NORMAL_MODE_KEYS: &[u8] = &[0x1c, 0x0e];
const NVIM_REDRAW_KEYS: &[u8] = &[0x0c];

#[derive(Default)]
struct State {
    pane_cwds: HashMap<PaneId, PathBuf>,
    known_terminal_panes: HashSet<PaneId>,
    pane_manifest: Option<PaneManifest>,
    pending_nvim_opens: HashMap<String, PendingNvimOpen>,
    next_request_id: u64,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::WriteToStdin,
            PermissionType::OpenTerminalsOrPlugins,
            PermissionType::RunCommands,
        ]);
        subscribe(&[EventType::PermissionRequestResult]);
    }

    fn update(&mut self, event: Event) -> bool {
        if event == Event::PermissionRequestResult(PermissionStatus::Granted) {
            set_selectable(false);
            subscribe(&[
                EventType::PaneUpdate,
                EventType::CwdChanged,
                EventType::HighlightClicked,
                EventType::RunCommandResult,
            ]);
            return false;
        }

        if let Event::PaneUpdate(pane_manifest) = &event {
            self.handle_pane_update(pane_manifest.clone());
            return false;
        }

        if let Event::CwdChanged(pane_id, cwd, _focused_client_ids) = &event {
            self.pane_cwds.insert(*pane_id, cwd.clone());
            Self::set_highlights_for_pane(*pane_id);
            return false;
        }

        if let Event::HighlightClicked {
            pane_id,
            pattern: _,
            matched_string,
            context: _,
        } = &event
        {
            self.handle_highlight_clicked(*pane_id, matched_string);
            return false;
        }

        if let Event::RunCommandResult(exit_code, _stdout, _stderr, context) = &event {
            self.handle_run_command_result(*exit_code, context);
        }

        false
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

impl State {
    fn handle_pane_update(&mut self, pane_manifest: PaneManifest) {
        let current_panes = pane_manifest
            .panes
            .values()
            .flat_map(|panes| panes.iter())
            .filter(|pane| is_open_terminal_pane(pane))
            .map(|pane| PaneId::Terminal(pane.id))
            .collect::<HashSet<_>>();
        for pane_id in &current_panes {
            if !self.known_terminal_panes.contains(pane_id) {
                if let Ok(cwd) = get_pane_cwd(*pane_id) {
                    self.pane_cwds.insert(*pane_id, cwd);
                }
                Self::set_highlights_for_pane(*pane_id);
            }
        }
        self.pane_cwds.retain(|pane_id, _cwd| current_panes.contains(pane_id));
        self.known_terminal_panes = current_panes;
        self.pane_manifest = Some(pane_manifest);
    }

    fn set_highlights_for_pane(pane_id: PaneId) {
        set_pane_regex_highlights(
            pane_id,
            vec![RegexHighlight {
                pattern: FILE_LOCATION_REGEX.to_owned(),
                style: HighlightStyle::None,
                layer: HighlightLayer::Tool,
                context: BTreeMap::new(),
                on_hover: true,
                bold: false,
                italic: false,
                underline: true,
                tooltip_text: Some("Open in nvim".to_owned()),
            }],
        );
    }

    fn handle_highlight_clicked(&mut self, source_pane_id: PaneId, matched_string: &str) {
        let Some(manifest) = self.pane_manifest.as_ref() else {
            return;
        };
        let Some(target) = FileTarget::parse(matched_string) else {
            return;
        };
        let Some(target) = panes_for_source_pane(manifest, source_pane_id)
            .filter(|tab_panes| terminal_pane_by_id(tab_panes, source_pane_id).is_some())
            .map(|_| target)
        else {
            return;
        };
        let source_cwd = get_pane_cwd(source_pane_id)
            .ok()
            .or_else(|| self.pane_cwds.get(&source_pane_id).cloned());
        let target = target.resolve(source_cwd.as_ref());

        self.check_file_exists_then_open(source_pane_id, target, source_cwd);
    }

    fn check_file_exists_then_open(&mut self, source_pane_id: PaneId, target: FileTarget, source_cwd: Option<PathBuf>) {
        let request_id = self.next_request_id.to_string();
        self.next_request_id = self.next_request_id.saturating_add(1);
        let pending_open = PendingNvimOpen {
            source_pane_id,
            target,
            source_cwd,
        };
        let mut context = BTreeMap::new();
        context.insert(CONTEXT_KIND.to_owned(), EXISTS_CHECK_KIND.to_owned());
        context.insert(CONTEXT_REQUEST_ID.to_owned(), request_id.clone());
        let path = pending_open.target.path.to_string_lossy().to_string();
        let command = ["/bin/sh", "-c", "test -e \"$1\"", "zop", &path];
        let cwd = pending_open.source_cwd.clone().unwrap_or_else(|| PathBuf::from("."));

        self.pending_nvim_opens.insert(request_id, pending_open);
        run_command_with_env_variables_and_cwd(&command, BTreeMap::new(), cwd, context);
    }

    fn handle_run_command_result(&mut self, exit_code: Option<i32>, context: &BTreeMap<String, String>) {
        if context.get(CONTEXT_KIND).is_none_or(|kind| kind != EXISTS_CHECK_KIND) {
            return;
        }
        let Some(request_id) = context.get(CONTEXT_REQUEST_ID) else {
            return;
        };
        let Some(pending_open) = self.pending_nvim_opens.remove(request_id) else {
            return;
        };
        if exit_code != Some(0) {
            return;
        }
        let Some(manifest) = self.pane_manifest.as_ref() else {
            return;
        };
        let Some(tab_panes) = panes_for_source_pane(manifest, pending_open.source_pane_id) else {
            return;
        };
        let Some(source_pane) = terminal_pane_by_id(tab_panes, pending_open.source_pane_id) else {
            return;
        };
        Self::open_target(tab_panes, source_pane, &pending_open);
    }

    fn open_target(tab_panes: &[PaneInfo], source_pane: &PaneInfo, pending_open: &PendingNvimOpen) {
        if let Some(target) = nearest_nvim_pane_with(tab_panes, source_pane, is_live_nvim_pane) {
            open_in_existing_nvim(target, &pending_open.target);
            return;
        }

        if let Some(target) = first_right_terminal_pane_with(tab_panes, source_pane, is_live_nvim_pane) {
            open_in_replaced_pane(target, &pending_open.target, pending_open.source_cwd.as_ref());
            return;
        }

        open_in_new_pane(
            pending_open.source_pane_id,
            &pending_open.target,
            pending_open.source_cwd.as_ref(),
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
const extern "C" fn host_run_plugin_command() {}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileTarget {
    path: PathBuf,
    line: usize,
    column: usize,
}

impl FileTarget {
    fn parse(input: &str) -> Option<Self> {
        let trimmed = input
            .trim()
            .trim_start_matches(['\'', '"', '(', '[', '{', '<'])
            .trim_end_matches(|ch: char| {
                ch.is_whitespace() || matches!(ch, ':' | ',' | '.' | '\'' | '"' | ')' | ']' | '}' | '>')
            });
        if trimmed.is_empty() {
            return None;
        }

        let Some((head, trailing_number)) = split_trailing_number(trimmed) else {
            return Self::new(trimmed, DEFAULT_LINE, DEFAULT_COLUMN);
        };

        let (path, line, column) = if let Some((path, line)) = split_trailing_number(head) {
            (path, line, trailing_number.max(DEFAULT_COLUMN))
        } else {
            (head, trailing_number, DEFAULT_COLUMN)
        };

        Self::new(path, line, column)
    }

    fn new(path: &str, line: usize, column: usize) -> Option<Self> {
        if path.is_empty() || line == 0 || path.contains("://") {
            return None;
        }
        Some(Self {
            path: PathBuf::from(path),
            line,
            column,
        })
    }

    fn resolve(self, cwd: Option<&PathBuf>) -> Self {
        let Self { mut path, line, column } = self;
        if !path.is_absolute()
            && let Some(cwd) = cwd
        {
            path = cwd.join(path);
        }

        Self { path, line, column }
    }

    fn terminal_cwd(&self, source_cwd: Option<&PathBuf>) -> PathBuf {
        source_cwd
            .cloned()
            .or_else(|| self.path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn cursor_arg(&self) -> String {
        format!("+call cursor({}, {})", self.line, self.column)
    }

    fn shell_command(&self, cwd: Option<&PathBuf>) -> String {
        let cd_prefix = cwd.map_or_else(String::new, |cwd| {
            format!("cd {} && ", shell_single_quoted_string(&cwd.to_string_lossy()))
        });
        format!(
            "{cd_prefix}nvim {} -- {}\r",
            shell_single_quoted_string(&self.cursor_arg()),
            shell_single_quoted_string(&self.path.to_string_lossy())
        )
    }

    fn edit_command(&self) -> String {
        let path = self.path.to_string_lossy();
        let mut quoted_path = String::from("'");
        for ch in path.chars() {
            if ch == '\'' {
                quoted_path.push('\'');
            }
            quoted_path.push(ch);
        }
        quoted_path.push('\'');

        format!(
            ":silent execute 'edit ' . fnameescape({quoted_path}) | call cursor({}, {}) | redraw!\r",
            self.line, self.column
        )
    }
}

#[derive(Clone, Debug)]
struct PendingNvimOpen {
    source_pane_id: PaneId,
    target: FileTarget,
    source_cwd: Option<PathBuf>,
}

fn panes_for_source_pane(manifest: &PaneManifest, source_pane_id: PaneId) -> Option<&[PaneInfo]> {
    let source_id = terminal_id(source_pane_id)?;
    manifest.panes.values().find_map(|panes| {
        panes
            .iter()
            .any(|pane| is_open_terminal_pane(pane) && pane.id == source_id)
            .then_some(panes.as_slice())
    })
}

fn terminal_pane_by_id(panes: &[PaneInfo], pane_id: PaneId) -> Option<&PaneInfo> {
    let terminal_id = terminal_id(pane_id)?;
    panes
        .iter()
        .find(|pane| is_open_terminal_pane(pane) && pane.id == terminal_id)
}

const fn terminal_id(pane_id: PaneId) -> Option<u32> {
    match pane_id {
        PaneId::Terminal(id) => Some(id),
        PaneId::Plugin(_) => None,
    }
}

const fn is_open_terminal_pane(pane: &PaneInfo) -> bool {
    !pane.is_plugin && !pane.exited && !pane.is_held && !pane.is_suppressed
}

fn nearest_nvim_pane_with<'a>(
    panes: &'a [PaneInfo],
    source_pane: &PaneInfo,
    mut is_nvim: impl FnMut(&PaneInfo) -> bool,
) -> Option<&'a PaneInfo> {
    panes
        .iter()
        .filter(|pane| is_open_terminal_pane(pane) && pane.id != source_pane.id && is_nvim(pane))
        .min_by_key(|pane| pane_distance(source_pane, pane))
}

fn is_live_nvim_pane(pane: &PaneInfo) -> bool {
    get_pane_running_command(PaneId::Terminal(pane.id))
        .ok()
        .is_some_and(|args| args.iter().any(|arg| is_nvim_command(arg)))
}

fn is_nvim_command(arg: &str) -> bool {
    let command = arg.rsplit('/').next().unwrap_or(arg);
    matches!(command, "nvim" | "nv")
}

const fn pane_distance(lhs: &PaneInfo, rhs: &PaneInfo) -> usize {
    lhs.pane_content_x
        .abs_diff(rhs.pane_content_x)
        .saturating_add(lhs.pane_content_y.abs_diff(rhs.pane_content_y))
}

fn first_right_terminal_pane_with<'a>(
    panes: &'a [PaneInfo],
    source_pane: &PaneInfo,
    mut is_nvim: impl FnMut(&PaneInfo) -> bool,
) -> Option<&'a PaneInfo> {
    panes
        .iter()
        .filter(|pane| {
            is_open_terminal_pane(pane)
                && pane.id != source_pane.id
                && !is_nvim(pane)
                && is_right_of(source_pane, pane)
                && vertically_overlaps(source_pane, pane)
        })
        .min_by_key(|pane| (pane.pane_content_x, pane.pane_content_y))
}

const fn is_right_of(source_pane: &PaneInfo, candidate: &PaneInfo) -> bool {
    let Some(source_right) = source_pane.pane_content_x.checked_add(source_pane.pane_content_columns) else {
        return false;
    };
    candidate.pane_content_x >= source_right
}

const fn vertically_overlaps(lhs: &PaneInfo, rhs: &PaneInfo) -> bool {
    let Some(lhs_bottom) = lhs.pane_content_y.checked_add(lhs.pane_content_rows) else {
        return false;
    };
    let Some(rhs_bottom) = rhs.pane_content_y.checked_add(rhs.pane_content_rows) else {
        return false;
    };
    lhs.pane_content_y < rhs_bottom && rhs.pane_content_y < lhs_bottom
}

fn split_trailing_number(input: &str) -> Option<(&str, usize)> {
    let colon_pos = input.rfind(':')?;
    let number = input.get(colon_pos.checked_add(1)?..)?;
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let head = input.get(..colon_pos)?;
    Some((head, number.parse().ok()?))
}

fn open_in_existing_nvim(target_pane: &PaneInfo, target: &FileTarget) {
    let pane_id = PaneId::Terminal(target_pane.id);
    focus_pane_with_id(pane_id, false, false);
    write_to_pane_id(NVIM_NORMAL_MODE_KEYS.to_vec(), pane_id);
    write_chars_to_pane_id(&target.edit_command(), pane_id);
    write_to_pane_id(NVIM_REDRAW_KEYS.to_vec(), pane_id);
}

fn open_in_replaced_pane(target_pane: &PaneInfo, target: &FileTarget, cwd: Option<&PathBuf>) {
    let pane_id = PaneId::Terminal(target_pane.id);
    focus_pane_with_id(pane_id, false, false);
    write_chars_to_pane_id(&target.shell_command(cwd), pane_id);
}

fn open_in_new_pane(source_pane_id: PaneId, target: &FileTarget, cwd: Option<&PathBuf>) {
    focus_pane_with_id(source_pane_id, false, false);
    if let Some(opened_pane_id) = open_terminal(target.terminal_cwd(cwd)) {
        move_pane_with_pane_id_in_direction(opened_pane_id, Direction::Right);
        focus_pane_with_id(opened_pane_id, false, false);
        resize_focused_pane_left_like_keymap();
        write_chars_to_pane_id(&target.shell_command(cwd), opened_pane_id);
    }
}

fn resize_focused_pane_left_like_keymap() {
    for _ in 0..RIGHT_PANE_RESIZE_LEFT_COUNT {
        resize_focused_pane_with_direction(Resize::Increase, Direction::Left);
    }
}

fn shell_single_quoted_string(input: &str) -> String {
    let mut output = String::from("'");
    for ch in input.chars() {
        if ch == '\'' {
            output.push_str("'\\''");
        } else {
            output.push(ch);
        }
    }
    output.push('\'');
    output
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("/tmp/foo.rs:42", Some(("/tmp/foo.rs", 42, 1)))]
    #[case("/tmp/foo.rs:42:9", Some(("/tmp/foo.rs", 42, 9)))]
    #[case("src/main.rs:7", Some(("src/main.rs", 7, 1)))]
    #[case("./src/main.rs:7", Some(("./src/main.rs", 7, 1)))]
    #[case(".env:7", Some((".env", 7, 1)))]
    #[case("Makefile:7", Some(("Makefile", 7, 1)))]
    #[case("src/main.rs", Some(("src/main.rs", 1, 1)))]
    #[case("Cargo", Some(("Cargo", 1, 1)))]
    #[case("(Cargo.toml),", Some(("Cargo.toml", 1, 1)))]
    #[case("https://example.test/file.rs", None)]
    fn test_file_target_parse_returns_target(#[case] input: &str, #[case] expected: Option<(&str, usize, usize)>) {
        let expected = expected.map(|(path, line, column)| FileTarget {
            path: PathBuf::from(path),
            line,
            column,
        });

        pretty_assertions::assert_eq!(FileTarget::parse(input), expected);
    }

    #[rstest]
    #[case("src/main.rs", Some("/repo"), "/repo/src/main.rs")]
    #[case("/tmp/main.rs", Some("/repo"), "/tmp/main.rs")]
    #[case("src/main.rs", None, "src/main.rs")]
    fn test_file_target_resolve_returns_resolved_target(
        #[case] path: &str,
        #[case] cwd: Option<&str>,
        #[case] expected_path: &str,
    ) {
        let target = FileTarget {
            path: PathBuf::from(path),
            line: 7,
            column: 3,
        };
        let cwd = cwd.map(PathBuf::from);

        pretty_assertions::assert_eq!(
            target.resolve(cwd.as_ref()),
            FileTarget {
                path: PathBuf::from(expected_path),
                line: 7,
                column: 3,
            }
        );
    }

    #[test]
    fn test_nearest_nvim_pane_multiple_nvim_panes_returns_closest() {
        let source = pane(1, "", None, 0, 0, 10, 10);
        let near = pane(2, "nvim", None, 11, 0, 10, 10);
        let far = pane(3, "nvim", None, 80, 0, 10, 10);
        pretty_assertions::assert_eq!(
            nearest_nvim_pane_with(&[source.clone(), far, near.clone()], &source, |pane| pane.title
                == "nvim")
            .map(|pane| pane.id),
            Some(near.id)
        );
    }

    #[test]
    fn test_first_right_terminal_pane_uses_first_overlapping_pane_to_right() {
        let source = pane(1, "", None, 0, 0, 10, 10);
        let below = pane(2, "", None, 10, 20, 10, 10);
        let first_right = pane(3, "", None, 10, 0, 10, 10);
        let second_right = pane(4, "", None, 25, 0, 10, 10);
        pretty_assertions::assert_eq!(
            first_right_terminal_pane_with(
                &[source.clone(), second_right, below, first_right.clone()],
                &source,
                |_| false
            )
            .map(|pane| pane.id),
            Some(first_right.id)
        );
    }

    #[rstest]
    #[case("/opt/homebrew/bin/nvim", true)]
    #[case("nv", true)]
    #[case("vim", false)]
    #[case("nvim-old", false)]
    #[case("preview", false)]
    fn test_is_nvim_command_matches_supported_binary_names(#[case] input: &str, #[case] expected: bool) {
        pretty_assertions::assert_eq!(is_nvim_command(input), expected);
    }

    #[rstest]
    #[case(
        FileTarget {
            path: PathBuf::from("/tmp/foo'bar.rs"),
            line: 12,
            column: 3,
        },
        ":silent execute 'edit ' . fnameescape('/tmp/foo''bar.rs') | call cursor(12, 3) | redraw!\r",
    )]
    fn test_file_target_edit_command_returns_nvim_command(#[case] target: FileTarget, #[case] expected: &str) {
        pretty_assertions::assert_eq!(target.edit_command(), expected);
    }

    #[rstest]
    #[case(
        FileTarget {
            path: PathBuf::from("/tmp/foo'bar.rs"),
            line: 12,
            column: 3,
        },
        None,
        "nvim '+call cursor(12, 3)' -- '/tmp/foo'\\''bar.rs'\r",
    )]
    #[case(
        FileTarget {
            path: PathBuf::from("/repo/src/main.rs"),
            line: 12,
            column: 3,
        },
        Some(PathBuf::from("/repo")),
        "cd '/repo' && nvim '+call cursor(12, 3)' -- '/repo/src/main.rs'\r",
    )]
    fn test_file_target_shell_command_returns_nvim_command(
        #[case] target: FileTarget,
        #[case] cwd: Option<PathBuf>,
        #[case] expected: &str,
    ) {
        pretty_assertions::assert_eq!(target.shell_command(cwd.as_ref()), expected);
    }

    fn pane(
        id: u32,
        title: &str,
        terminal_command: Option<&str>,
        x: usize,
        y: usize,
        columns: usize,
        rows: usize,
    ) -> PaneInfo {
        PaneInfo {
            id,
            is_plugin: false,
            title: title.to_owned(),
            terminal_command: terminal_command.map(str::to_owned),
            pane_content_x: x,
            pane_content_y: y,
            pane_content_columns: columns,
            pane_content_rows: rows,
            ..PaneInfo::default()
        }
    }
}
