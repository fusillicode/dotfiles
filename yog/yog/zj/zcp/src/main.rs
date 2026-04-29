use std::collections::BTreeMap;

use zellij_tile::prelude::*;

const SMART_COPY_PIPE: &str = "smart-copy";
const DEFAULT_MESSAGE_PLUGIN_PIPE_SUFFIX: &str = "/zcp.wasm";

#[derive(Default)]
struct State;

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ReadPaneContents,
            PermissionType::WriteToClipboard,
        ]);
        subscribe(&[EventType::PermissionRequestResult]);
    }

    fn update(&mut self, event: Event) -> bool {
        if event == Event::PermissionRequestResult(PermissionStatus::Granted) {
            set_selectable(false);
        }
        false
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        if !is_smart_copy_pipe_name(&pipe_message.name) {
            return false;
        }

        copy_selection(selected_text_from_focused_pane(), copy_to_clipboard);
        false
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
const extern "C" fn host_run_plugin_command() {}

fn selected_text_from_focused_pane() -> Option<String> {
    let Ok((_tab_idx, pane_id)) = get_focused_pane_info() else {
        return None;
    };
    let Ok(contents) = get_pane_scrollback(pane_id, false) else {
        return None;
    };
    contents.get_selected_text()
}

fn is_smart_copy_pipe_name(name: &str) -> bool {
    name == SMART_COPY_PIPE || name.ends_with(DEFAULT_MESSAGE_PLUGIN_PIPE_SUFFIX)
}

fn copy_selection(selection: Option<String>, copy: impl FnOnce(String)) -> bool {
    let Some(selection) = selection else {
        return false;
    };

    copy(normalize_selection(&selection));
    true
}

fn normalize_selection(selection: &str) -> String {
    selection.split('\n').map(str::trim).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_selection_joins_indented_flag_continuations() {
        let actual = normalize_selection("cargo test\n  --package foo\n  -- --nocapture");

        pretty_assertions::assert_eq!(actual, "cargo test --package foo -- --nocapture");
    }

    #[test]
    fn test_normalize_selection_trims_surrounding_indentation_and_whitespace() {
        let actual = normalize_selection("  cargo test  \n    --all-targets\t");

        pretty_assertions::assert_eq!(actual, "cargo test --all-targets");
    }

    #[test]
    fn test_normalize_selection_preserves_interior_spaces_within_line() {
        let actual = normalize_selection("printf 'a  b'\n  --flag=value");

        pretty_assertions::assert_eq!(actual, "printf 'a  b' --flag=value");
    }

    #[test]
    fn test_normalize_selection_single_line_stays_single_line() {
        let actual = normalize_selection("  cargo test  ");

        pretty_assertions::assert_eq!(actual, "cargo test");
    }

    #[test]
    fn test_normalize_selection_blank_line_still_copies() {
        let actual = normalize_selection("cargo test\n\n  --all-targets");

        pretty_assertions::assert_eq!(actual, "cargo test  --all-targets");
    }

    #[test]
    fn test_normalize_selection_all_whitespace_still_copies_empty_text() {
        let actual = normalize_selection("  \n\t");

        pretty_assertions::assert_eq!(actual, " ");
    }

    #[test]
    fn test_copy_selection_valid_smart_copy_selection_writes_clipboard_text() {
        let mut copied = None;
        let changed = copy_selection(Some("cargo test\n  --all-targets".to_string()), |text| {
            copied = Some(text);
        });

        assert!(changed);
        pretty_assertions::assert_eq!(copied.as_deref(), Some("cargo test --all-targets"));
    }

    #[test]
    fn test_copy_selection_missing_selection_is_no_op() {
        let mut copied = None;
        let changed = copy_selection(None, |text| {
            copied = Some(text);
        });

        assert!(!changed);
        pretty_assertions::assert_eq!(copied, None);
    }

    #[test]
    fn test_copy_selection_blank_line_selection_writes_clipboard_text() {
        let mut copied = None;
        let changed = copy_selection(Some("cargo test\n\n  --all-targets".to_string()), |text| {
            copied = Some(text);
        });

        assert!(changed);
        pretty_assertions::assert_eq!(copied.as_deref(), Some("cargo test  --all-targets"));
    }

    #[test]
    fn test_is_smart_copy_pipe_name_accepts_explicit_name() {
        assert!(is_smart_copy_pipe_name("smart-copy"));
    }

    #[test]
    fn test_is_smart_copy_pipe_name_accepts_message_plugin_default_name() {
        assert!(is_smart_copy_pipe_name("file:~/.config/zellij/plugins/zcp.wasm"));
    }

    #[test]
    fn test_is_smart_copy_pipe_name_rejects_unrelated_pipe() {
        assert!(!is_smart_copy_pipe_name("other"));
    }
}
