use ytil_agents::agent::Agent;
use zellij_tile::prelude::PaneInfo;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FocusedPane {
    pub id: u32,
    pub label: Option<FocusedPaneLabel>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FocusedPaneLabel {
    TerminalCommand(String),
    Title(String),
}

pub const fn is_displayable_terminal_pane(pane: &PaneInfo) -> bool {
    !pane.is_plugin && !pane.is_suppressed
}

pub fn focused_pane_from_pane_info(pane: &PaneInfo) -> Option<FocusedPane> {
    if !is_displayable_terminal_pane(pane) {
        return None;
    }

    Some(FocusedPane {
        id: pane.id,
        label: pane
            .terminal_command
            .as_deref()
            .and_then(parse_running_command)
            .map(FocusedPaneLabel::TerminalCommand)
            .or_else(|| title_label_from_title(&pane.title).map(FocusedPaneLabel::Title)),
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

pub fn parse_running_command(command: &str) -> Option<String> {
    let executable = command.split_whitespace().next()?;
    let executable = command_name(executable);
    if is_shell(executable) {
        return None;
    }
    Some(executable.to_string())
}

pub fn agent_from_command_args(args: &[String]) -> Option<Agent> {
    let command = args.first()?;
    Agent::detect(command_name(command)).or_else(|| Agent::detect(&args.join(" ")))
}

pub fn label_from_command_args(args: &[String]) -> Option<String> {
    let executable = args.first().map(String::as_str).map(command_name)?;
    if is_shell(executable) {
        return None;
    }
    Some(executable.to_string())
}

pub fn title_label_from_title(title: &str) -> Option<String> {
    let title = title.trim();
    (!title.is_empty()
        && !title.starts_with('~')
        && !title.starts_with('/')
        && title != "Pane"
        && !title.starts_with("Pane "))
    .then(|| ytil_tui::display_fixed_width(title, 8))
}

pub fn command_name(command: &str) -> &str {
    command.rsplit('/').next().unwrap_or(command)
}

fn is_shell(executable: &str) -> bool {
    executable.is_empty() || matches!(executable, "zsh" | "bash" | "fish")
}

#[cfg(test)]
mod tests {
    use zellij_tile::prelude::PaneInfo;

    use super::*;

    fn plugin_pane(id: u32) -> PaneInfo {
        PaneInfo {
            id,
            is_plugin: true,
            ..Default::default()
        }
    }

    fn terminal_pane_with_command(id: u32, is_focused: bool, command: &str) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            terminal_command: Some(command.to_string()),
            ..Default::default()
        }
    }

    fn terminal_pane_with_title(id: u32, is_focused: bool, title: &str) -> PaneInfo {
        PaneInfo {
            id,
            is_focused,
            title: title.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_is_displayable_terminal_pane_matches_ppick_visibility() {
        assert!(is_displayable_terminal_pane(&terminal_pane_with_command(
            42, true, "zsh"
        )));
        assert!(is_displayable_terminal_pane(&PaneInfo {
            exited: true,
            ..terminal_pane_with_command(43, false, "gkg")
        }));
        assert!(is_displayable_terminal_pane(&PaneInfo {
            is_held: true,
            ..terminal_pane_with_command(44, false, "gkg")
        }));
        assert!(!is_displayable_terminal_pane(&plugin_pane(7)));
        assert!(!is_displayable_terminal_pane(&PaneInfo {
            is_suppressed: true,
            ..terminal_pane_with_command(45, false, "zsh")
        }));
    }

    #[test]
    fn test_parse_running_command_filters_shells() {
        pretty_assertions::assert_eq!(parse_running_command("/bin/zsh"), None);
        pretty_assertions::assert_eq!(parse_running_command("/usr/bin/cargo test"), Some("cargo".to_string()));
    }

    #[test]
    fn test_detected_agent_from_pane_info_detects_wrapped_codex_terminal_command() {
        let pane = terminal_pane_with_command(42, true, "/bin/zsh -lc codex");
        let focused_pane = FocusedPane { id: 42, label: None };

        pretty_assertions::assert_eq!(detected_agent_from_pane_info(&pane, &focused_pane), Some(Agent::Codex));
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

        pretty_assertions::assert_eq!(detected_agent_from_pane_info(&pane, &focused_pane), None);
    }

    #[test]
    fn test_detected_agent_from_pane_info_detects_codex_from_title_fallback() {
        let pane = terminal_pane_with_title(42, true, "codex");
        let focused_pane = FocusedPane {
            id: 42,
            label: Some(FocusedPaneLabel::Title("codex".to_string())),
        };

        pretty_assertions::assert_eq!(detected_agent_from_pane_info(&pane, &focused_pane), Some(Agent::Codex));
    }

    #[test]
    fn test_title_label_from_title_filters_paths() {
        pretty_assertions::assert_eq!(title_label_from_title("/tmp/project"), None);
        pretty_assertions::assert_eq!(title_label_from_title("gkg"), Some("gkg".to_string()));
        pretty_assertions::assert_eq!(title_label_from_title("Cursor Agent"), Some("Cursor …".to_string()));
    }
}
