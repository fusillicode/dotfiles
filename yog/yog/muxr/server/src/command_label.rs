use std::path::Path;

/// A terminal title classified for the tab bar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalTitle {
    /// Shell command-label text to show below the cwd row.
    pub command_label: Option<String>,
    /// Cwd text to show in the path row and persist in pane metadata.
    pub cwd: Option<String>,
}

/// Classify an OSC terminal title as either cwd metadata or command-label text.
pub fn classify_terminal_title(title: Option<&str>, cwd: &str) -> TerminalTitle {
    let home = std::env::var("HOME").ok();
    self::classify_terminal_title_with_home(title, cwd, home.as_deref())
}

fn classify_terminal_title_with_home(title: Option<&str>, cwd: &str, home: Option<&str>) -> TerminalTitle {
    let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) else {
        return TerminalTitle {
            command_label: None,
            cwd: None,
        };
    };
    let title_cwd = self::cwd_from_title(title, cwd, home);
    if self::is_shell_title(title) || title_cwd.is_some() || self::is_ignored_title(title) {
        return TerminalTitle {
            command_label: None,
            cwd: title_cwd,
        };
    }

    TerminalTitle {
        command_label: Some(title.to_owned()),
        cwd: None,
    }
}

fn is_shell_title(title: &str) -> bool {
    let Some(first_word) = title.split_whitespace().next() else {
        return true;
    };
    let command = first_word.rsplit('/').next().unwrap_or(first_word);
    matches!(command, "bash" | "fish" | "sh" | "zsh")
}

fn cwd_from_title(title: &str, cwd: &str, home: Option<&str>) -> Option<String> {
    let cwd = cwd.trim();
    if title == cwd {
        return Some(title.to_owned());
    }
    if title == "~" {
        return Some(title.to_owned());
    }
    if self::home_abbreviated_title_is_dir(title, home) || self::absolute_title_is_dir(title) {
        return Some(title.to_owned());
    }
    None
}

fn home_abbreviated_title_is_dir(title: &str, home: Option<&str>) -> bool {
    let Some(rest) = title.strip_prefix("~/").filter(|rest| !rest.is_empty()) else {
        return false;
    };
    let Some(home) = home.map(str::trim).filter(|home| !home.is_empty()) else {
        return false;
    };
    Path::new(home).join(rest).is_dir()
}

fn absolute_title_is_dir(title: &str) -> bool {
    let path = Path::new(title);
    path.is_absolute() && path.is_dir()
}

fn is_ignored_title(title: &str) -> bool {
    title == "Pane" || title.starts_with("Pane ")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::alias("gst", "/work/project", Some("gst"))]
    #[case::command_with_args("cargo test", "/work/project", Some("cargo test"))]
    #[case::custom_command_with_args("gkg server", "/work/project", Some("gkg server"))]
    #[case::relative_command("./script.sh", "/work/project", Some("./script.sh"))]
    #[case::relative_command_with_args("../bin/gkg server", "/work/project", Some("../bin/gkg server"))]
    #[case::absolute_command_with_args("/usr/bin/cargo test", "/work/project", Some("/usr/bin/cargo test"))]
    #[case::home_command_with_args("~/bin/tool run", "/work/project", Some("~/bin/tool run"))]
    #[case::absolute_shell("/bin/zsh", "/work/project", None)]
    #[case::shell_with_args("zsh -l", "/work/project", None)]
    #[case::matching_cwd("/work/project", "/work/project", None)]
    #[case::home_cwd("~", "/old/project", None)]
    #[case::empty("", "/work/project", None)]
    #[case::default_pane_title("Pane 1", "/work/project", None)]
    fn test_classify_terminal_title_when_title_is_seen_returns_command_label(
        #[case] title: &str,
        #[case] cwd: &str,
        #[case] expected: Option<&str>,
    ) {
        pretty_assertions::assert_eq!(
            classify_terminal_title_with_home(Some(title), cwd, Some("/Users/gianlu")).command_label,
            expected.map(ToOwned::to_owned)
        );
    }

    #[test]
    fn test_classify_terminal_title_when_title_is_home_cwd_updates_cwd() -> rootcause::Result<()> {
        let home = tempfile::Builder::new().prefix("muxr-home.").tempdir()?;

        pretty_assertions::assert_eq!(
            classify_terminal_title_with_home(Some("~"), "/old/project", Some(home.path().to_string_lossy().as_ref())),
            TerminalTitle {
                command_label: None,
                cwd: Some("~".to_owned()),
            },
        );
        Ok(())
    }

    #[test]
    fn test_classify_terminal_title_when_home_path_is_existing_dir_updates_cwd() -> rootcause::Result<()> {
        let home = tempfile::Builder::new().prefix("muxr-home.").tempdir()?;
        std::fs::create_dir(home.path().join("My Project"))?;

        pretty_assertions::assert_eq!(
            classify_terminal_title_with_home(
                Some("~/My Project"),
                "/old/project",
                Some(home.path().to_string_lossy().as_ref())
            ),
            TerminalTitle {
                command_label: None,
                cwd: Some("~/My Project".to_owned()),
            },
        );
        Ok(())
    }

    #[test]
    fn test_classify_terminal_title_when_home_path_is_file_returns_command_label() -> rootcause::Result<()> {
        let home = tempfile::Builder::new().prefix("muxr-home.").tempdir()?;
        std::fs::create_dir(home.path().join("bin"))?;
        std::fs::write(home.path().join("bin").join("tool"), b"")?;

        pretty_assertions::assert_eq!(
            classify_terminal_title_with_home(
                Some("~/bin/tool"),
                "/old/project",
                Some(home.path().to_string_lossy().as_ref())
            ),
            TerminalTitle {
                command_label: Some("~/bin/tool".to_owned()),
                cwd: None,
            },
        );
        Ok(())
    }
}
