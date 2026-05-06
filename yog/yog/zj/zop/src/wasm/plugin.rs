use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

use zellij_tile::prelude::*;

use crate::wasm::file_target::FileTarget;
use crate::wasm::file_target::FileTargetReconstructed;

const FILE_LOCATION_REGEX: &str = r"\S+";
const EXISTS_CHECK_KIND: &str = "zop-exists-check";
const CONTEXT_KIND: &str = "kind";
const CONTEXT_REQUEST_ID: &str = "request_id";
const NVIM_NORMAL_MODE_KEYS: &[u8] = &[0x1c, 0x0e];
const NVIM_REDRAW_KEYS: &[u8] = &[0x0c];

#[derive(Default)]
pub struct State {
    pane_cwds: HashMap<PaneId, PathBuf>,
    known_terminal_panes: HashSet<PaneId>,
    pane_manifest: Option<PaneManifest>,
    pending_nvim_opens: HashMap<String, PendingNvimOpen>,
    next_request_id: u64,
}

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::WriteToStdin,
            PermissionType::OpenTerminalsOrPlugins,
            PermissionType::RunCommands,
            PermissionType::ReadPaneContents,
        ]);
        subscribe(&[EventType::PermissionRequestResult]);
    }

    fn update(&mut self, event: Event) -> bool {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "zop only subscribes to a small Event subset"
        )]
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                set_selectable(false);
                subscribe(&[
                    EventType::PaneUpdate,
                    EventType::CwdChanged,
                    EventType::HighlightClicked,
                    EventType::RunCommandResult,
                ]);
            }
            Event::PaneUpdate(pane_manifest) => self.handle_pane_update(pane_manifest),
            Event::CwdChanged(pane_id, cwd, _focused_client_ids) => {
                self.pane_cwds.insert(pane_id, cwd);
                set_highlights_for_pane(pane_id);
            }
            Event::HighlightClicked {
                pane_id,
                matched_string,
                ..
            } => self.handle_highlight_clicked(pane_id, &matched_string),
            Event::RunCommandResult(exit_code, _stdout, _stderr, context) => {
                let _ = self.handle_run_command_result(exit_code, &context);
            }
            _ => {}
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
                set_highlights_for_pane(*pane_id);
            }
        }
        self.pane_cwds.retain(|pane_id, _cwd| current_panes.contains(pane_id));
        self.known_terminal_panes = current_panes;
        self.pane_manifest = Some(pane_manifest);
    }

    fn handle_highlight_clicked(&mut self, source_pane_id: PaneId, matched_string: &str) {
        let Some(manifest) = self.pane_manifest.as_ref() else {
            return;
        };
        let Some(tab_panes) = panes_for_source_pane(manifest, source_pane_id) else {
            return;
        };
        if terminal_pane_by_id(tab_panes, source_pane_id).is_none() {
            return;
        }
        let target = match get_pane_scrollback(source_pane_id, false)
            .ok()
            .map(|contents| FileTargetReconstructed::from_lines(&contents.viewport, matched_string))
        {
            Some(FileTargetReconstructed::Unique(target)) => target,
            Some(FileTargetReconstructed::Ambiguous) => return,
            Some(FileTargetReconstructed::NoMatch) | None => {
                let Some(target) = FileTarget::parse(matched_string) else {
                    return;
                };
                target
            }
        };
        let source_cwd = get_pane_cwd(source_pane_id)
            .ok()
            .or_else(|| self.pane_cwds.get(&source_pane_id).cloned());
        let target = target.resolve(source_cwd.as_ref());

        self.check_file_exists_then_open(source_pane_id, target, source_cwd);
    }

    fn handle_run_command_result(&mut self, exit_code: Option<i32>, context: &BTreeMap<String, String>) -> Option<()> {
        (context.get(CONTEXT_KIND)? == EXISTS_CHECK_KIND).then_some(())?;
        let request_id = context.get(CONTEXT_REQUEST_ID)?;
        let pending_open = self.pending_nvim_opens.remove(request_id)?;
        (exit_code == Some(0)).then_some(())?;
        let manifest = self.pane_manifest.as_ref()?;
        let tab_panes = panes_for_source_pane(manifest, pending_open.source_pane_id)?;
        let source_pane = terminal_pane_by_id(tab_panes, pending_open.source_pane_id)?;

        if let Some(target) = nearest_nvim_pane_with(tab_panes, source_pane, is_live_nvim_pane) {
            open_in_existing_nvim(target, &pending_open.target);
            return Some(());
        }

        if let Some(target) = first_right_terminal_pane_with(tab_panes, source_pane, is_live_nvim_pane) {
            open_in_replaced_pane(target, &pending_open.target, pending_open.source_cwd.as_ref());
            return Some(());
        }

        focus_pane_with_id(pending_open.source_pane_id, false, false);
        if let Some(opened_pane_id) = open_terminal(
            pending_open
                .source_cwd
                .clone()
                .or_else(|| pending_open.target.parent_path().map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from(".")),
        ) {
            move_pane_with_pane_id_in_direction(opened_pane_id, Direction::Right);
            focus_pane_with_id(opened_pane_id, false, false);
            for _ in 0..2 {
                resize_focused_pane_with_direction(Resize::Increase, Direction::Left);
            }
            write_chars_to_pane_id(
                &pending_open.target.shell_cmd(pending_open.source_cwd.as_ref()),
                opened_pane_id,
            );
        }

        Some(())
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
        let path = pending_open.target.path().to_string_lossy().to_string();
        let command = ["/bin/sh", "-c", "test -e \"$1\"", "zop", &path];
        let cwd = pending_open.source_cwd.clone().unwrap_or_else(|| PathBuf::from("."));

        self.pending_nvim_opens.insert(request_id, pending_open);
        run_command_with_env_variables_and_cwd(&command, BTreeMap::new(), cwd, context);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
const extern "C" fn host_run_plugin_command() {}

#[derive(Clone, Debug)]
struct PendingNvimOpen {
    source_pane_id: PaneId,
    target: FileTarget,
    source_cwd: Option<PathBuf>,
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

fn open_in_existing_nvim(target_pane: &PaneInfo, target: &FileTarget) {
    let pane_id = PaneId::Terminal(target_pane.id);
    focus_pane_with_id(pane_id, false, false);
    write_to_pane_id(NVIM_NORMAL_MODE_KEYS.to_vec(), pane_id);
    write_chars_to_pane_id(&target.edit_cmd(), pane_id);
    write_to_pane_id(NVIM_REDRAW_KEYS.to_vec(), pane_id);
}

fn open_in_replaced_pane(target_pane: &PaneInfo, target: &FileTarget, cwd: Option<&PathBuf>) {
    let pane_id = PaneId::Terminal(target_pane.id);
    focus_pane_with_id(pane_id, false, false);
    write_chars_to_pane_id(&target.shell_cmd(cwd), pane_id);
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use zellij_tile::prelude::PaneInfo;

    use crate::wasm::plugin::first_right_terminal_pane_with;
    use crate::wasm::plugin::is_nvim_command;
    use crate::wasm::plugin::nearest_nvim_pane_with;

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
