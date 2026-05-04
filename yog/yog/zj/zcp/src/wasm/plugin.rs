use std::collections::BTreeMap;

use zellij_tile::prelude::*;

const ZCP_PIPE: &str = "zcp";

#[derive(Default)]
pub struct State;

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
        if pipe_message.name != ZCP_PIPE {
            return false;
        }

        let Some(selection) = get_selected_text_from_focused_pane(get_focused_pane_info, get_pane_scrollback) else {
            return false;
        };

        copy_to_clipboard(selection);
        false
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[unsafe(no_mangle)]
const extern "C" fn host_run_plugin_command() {}

fn get_selected_text_from_focused_pane(
    get_focused_pane_info: impl FnOnce() -> Result<(usize, PaneId), String>,
    get_pane_scrollback: impl FnOnce(PaneId, bool) -> Result<PaneContents, String>,
) -> Option<String> {
    let Ok((_tab_idx, pane_id)) = get_focused_pane_info() else {
        return None;
    };
    let Ok(contents) = get_pane_scrollback(pane_id, false) else {
        return None;
    };
    contents
        .get_selected_text()
        .map(|selection| selection.split('\n').map(str::trim).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use zellij_utils::position::Position;

    use super::*;

    #[rstest]
    #[case(
        vec!["cargo test", "  --package foo", "  -- --nocapture"],
        Position::new(0, 0),
        Position::new(2, 16),
        Some("cargo test --package foo -- --nocapture"),
    )]
    #[case(
        vec!["  cargo test  ", "    --all-targets\t"],
        Position::new(0, 0),
        Position::new(1, 18),
        Some("cargo test --all-targets"),
    )]
    #[case(
        vec!["printf 'a  b'", "  --flag=value"],
        Position::new(0, 0),
        Position::new(1, 14),
        Some("printf 'a  b' --flag=value"),
    )]
    fn test_get_selected_text_from_focused_pane_returns_normalized_selected_text(
        #[case] viewport: Vec<&str>,
        #[case] selection_start: Position,
        #[case] selection_end: Position,
        #[case] expected: Option<&str>,
    ) {
        let pane_id = PaneId::Terminal(1);
        let pane_contents = PaneContents::new(
            viewport.into_iter().map(str::to_string).collect(),
            selection_start,
            selection_end,
        );
        let actual = get_selected_text_from_focused_pane(
            || Ok((0, pane_id)),
            |actual_pane_id, get_full_scrollback| {
                pretty_assertions::assert_eq!(actual_pane_id, pane_id);
                pretty_assertions::assert_eq!(get_full_scrollback, false);
                Ok(pane_contents)
            },
        );

        pretty_assertions::assert_eq!(actual.as_deref(), expected);
    }

    #[test]
    fn test_get_selected_text_from_focused_pane_returns_none_when_no_pane_is_focused() {
        let mut read_scrollback = false;
        let actual = get_selected_text_from_focused_pane(
            || Err("missing focused pane".to_string()),
            |_pane_id, _get_full_scrollback| {
                read_scrollback = true;
                Ok(PaneContents::default())
            },
        );

        pretty_assertions::assert_eq!(actual, None);
        pretty_assertions::assert_eq!(read_scrollback, false);
    }

    #[test]
    fn test_get_selected_text_from_focused_pane_returns_none_when_no_text_is_selected() {
        let actual = get_selected_text_from_focused_pane(
            || Ok((0, PaneId::Terminal(1))),
            |_pane_id, _get_full_scrollback| Ok(PaneContents::default()),
        );

        pretty_assertions::assert_eq!(actual, None);
    }
}
