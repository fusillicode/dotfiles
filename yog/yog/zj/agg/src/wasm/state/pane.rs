use ytil_agents::agent::Agent;
use zellij_tile::prelude::PaneInfo;

use super::current_tab::FocusedPane;
use super::current_tab::FocusedPaneLabel;

pub fn focused_pane_from_pane_info(pane: &PaneInfo) -> Option<FocusedPane> {
    if pane.is_plugin || pane.exited || pane.is_held {
        return None;
    }

    Some(FocusedPane {
        id: pane.id,
        label: pane
            .terminal_command
            .as_deref()
            .and_then(parse_running_command)
            .map(FocusedPaneLabel::TerminalCommand)
            .or_else(|| focused_pane_title_label(pane).map(FocusedPaneLabel::Title)),
    })
}

pub fn detected_agent_from_pane_info(pane: &PaneInfo, focused_pane: &FocusedPane) -> Option<Agent> {
    if let Some(command) = pane.terminal_command.as_deref().map(str::trim)
        && !command.is_empty()
    {
        if let Some(running_command) = parse_running_command(command)
            && let Some(agent) = Agent::detect(&running_command)
        {
            return Some(agent);
        }

        if let Some(agent) = Agent::detect(command) {
            return Some(agent);
        }
        return None;
    }

    match focused_pane.label.as_ref() {
        Some(FocusedPaneLabel::TerminalCommand(label) | FocusedPaneLabel::Title(label)) => Agent::detect(label),
        None => None,
    }
}

pub fn focused_pane_title_label(pane: &PaneInfo) -> Option<String> {
    if pane.exited || pane.is_held {
        return None;
    }
    let title = pane.title.trim();
    (!title.is_empty()
        && !title.starts_with('~')
        && !title.starts_with('/')
        && title != "Pane"
        && !title.starts_with("Pane "))
    .then(|| ytil_tui::display_fixed_width(title, 8))
}

pub fn parse_running_command(command: &str) -> Option<String> {
    let executable = command.split_whitespace().next()?;
    let executable = executable.rsplit('/').next().unwrap_or(executable);
    if executable.is_empty() || matches!(executable, "zsh" | "bash" | "fish") {
        return None;
    }
    Some(executable.to_string())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::wasm::state::test_support::*;

    #[test]
    fn test_parse_running_command_filters_shells() {
        assert_eq!(parse_running_command("/bin/zsh"), None);
        assert_eq!(parse_running_command("/usr/bin/cargo test"), Some("cargo".to_string()));
    }

    #[test]
    fn test_detected_agent_from_pane_info_detects_wrapped_codex_terminal_command() {
        let pane = terminal_pane_with_command(42, true, "/bin/zsh -lc codex");
        let focused_pane = FocusedPane { id: 42, label: None };

        assert_eq!(detected_agent_from_pane_info(&pane, &focused_pane), Some(Agent::Codex));
    }

    #[test]
    fn test_detected_agent_from_pane_info_ignores_title_when_terminal_command_exists() {
        let pane = PaneInfo {
            id: 42,
            is_focused: true,
            terminal_command: Some("/bin/zsh".to_string()),
            title: "Cursor Agent".to_string(),
            ..Default::default()
        };
        let focused_pane = FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::Title("Cursor …".to_string())),
        };

        assert_eq!(detected_agent_from_pane_info(&pane, &focused_pane), None);
    }

    #[test]
    fn test_detected_agent_from_pane_info_detects_codex_from_title_fallback() {
        let pane = terminal_pane_with_title(42, true, "codex");
        let focused_pane = FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::Title("codex".to_string())),
        };

        assert_eq!(detected_agent_from_pane_info(&pane, &focused_pane), Some(Agent::Codex));
    }

    #[test]
    fn test_focused_pane_title_label_filters_paths() {
        assert_eq!(
            focused_pane_title_label(&terminal_pane_with_title(42, true, "/tmp/project")),
            None
        );
        assert_eq!(
            focused_pane_title_label(&terminal_pane_with_title(42, true, "Cursor Agent")),
            Some("Cursor …".to_string())
        );
    }
}
