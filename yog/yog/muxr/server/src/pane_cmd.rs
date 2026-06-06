use std::path::Path;

use ytil_agents::agent::Agent;

use crate::pty::PtyHandle;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneProcess {
    pub name: Option<String>,
    pub path: Option<String>,
    pub pid: u32,
}

impl PaneProcess {
    fn cmd(&self) -> Option<PaneCmd> {
        let executable = self::process_executable(self.path.as_deref())
            .or_else(|| self::process_executable(self.name.as_deref()))?;
        Some(PaneCmd {
            executable: executable.to_owned(),
            path: self.path.clone(),
            pid: self.pid,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneCmd {
    pub executable: String,
    pub path: Option<String>,
    pub pid: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PaneCmdSnapshot {
    pub fg_process_group: Option<u32>,
    pub fg_process_group_leader: Option<PaneProcess>,
    pub shell_pid: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaneCmdObservation {
    FgCmd { cmd: PaneCmd },
    Shell,
    Unknown { reason: PaneCmdUnknownReason },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneCmdUnknownReason {
    FgProcessHasNoExecutable,
    MissingFgProcess,
    MissingFgProcessGroup,
}

pub fn observe_pane_cmd(snapshot: &PaneCmdSnapshot) -> PaneCmdObservation {
    if snapshot.fg_process_group.is_none() {
        return PaneCmdObservation::Unknown {
            reason: PaneCmdUnknownReason::MissingFgProcessGroup,
        };
    }

    if let Some(process) = &snapshot.fg_process_group_leader {
        return self::observe_process(process, snapshot.shell_pid);
    }

    PaneCmdObservation::Unknown {
        reason: PaneCmdUnknownReason::MissingFgProcess,
    }
}

pub fn snapshot_from_pty_handle(handle: &PtyHandle) -> rootcause::Result<PaneCmdSnapshot> {
    let shell_pid = handle.process_id()?;
    let fg_process_group = handle.fg_process_group()?;
    let fg_process_group_leader = fg_process_group.and_then(self::process_info);

    Ok(PaneCmdSnapshot {
        fg_process_group,
        fg_process_group_leader,
        shell_pid,
    })
}

pub fn agent_for_cmd(cmd: &PaneCmd) -> Option<Agent> {
    match cmd.executable.as_str() {
        "claude" | "claude-code" => Some(Agent::Claude),
        "codex" | "codex-aarch64-apple-darwin" | "codex-x86_64-apple-darwin" => Some(Agent::Codex),
        "cursor" | "cursor-agent" => Some(Agent::Cursor),
        "gemini" => Some(Agent::Gemini),
        "opencode" => Some(Agent::Opencode),
        _ if self::is_claude_versioned_runtime(cmd) => Some(Agent::Claude),
        _ if self::is_cursor_agent_node(cmd) => Some(Agent::Cursor),
        _ => None,
    }
}

fn is_claude_versioned_runtime(cmd: &PaneCmd) -> bool {
    cmd.path
        .as_deref()
        .is_some_and(|path| path.contains("/claude/versions/"))
}

fn is_cursor_agent_node(cmd: &PaneCmd) -> bool {
    cmd.executable == "node"
        && cmd
            .path
            .as_deref()
            .is_some_and(|path| path.contains("/cursor-agent/versions/"))
}

fn observe_process(process: &PaneProcess, shell_pid: Option<u32>) -> PaneCmdObservation {
    if shell_pid.is_some_and(|shell_pid| process.pid == shell_pid) {
        return PaneCmdObservation::Shell;
    }

    let Some(cmd) = process.cmd() else {
        return PaneCmdObservation::Unknown {
            reason: PaneCmdUnknownReason::FgProcessHasNoExecutable,
        };
    };

    PaneCmdObservation::FgCmd { cmd }
}

fn process_executable(raw: Option<&str>) -> Option<&str> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }

    Path::new(raw).file_name().and_then(|name| name.to_str()).or(Some(raw))
}

fn process_info(pid: u32) -> Option<PaneProcess> {
    let Ok(pid_i32) = i32::try_from(pid) else {
        return None;
    };
    Some(PaneProcess {
        // Process lookup can race with fg-process exit. Treat misses as absent metadata; observation stays
        // Unknown instead of clearing a live agent from one failed sample.
        name: libproc::proc_pid::name(pid_i32).ok(),
        path: libproc::proc_pid::pidpath(pid_i32).ok(),
        pid,
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn test_observe_pane_cmd_when_fg_leader_is_shell_returns_shell() {
        let observation = observe_pane_cmd(&PaneCmdSnapshot {
            fg_process_group: Some(9322),
            fg_process_group_leader: Some(self::process(9322, "zsh")),
            shell_pid: Some(9322),
        });

        pretty_assertions::assert_eq!(observation, PaneCmdObservation::Shell);
    }

    #[test]
    fn test_observe_pane_cmd_when_fg_leader_is_codex_returns_fg_cmd() {
        let observation = observe_pane_cmd(&PaneCmdSnapshot {
            fg_process_group: Some(9400),
            fg_process_group_leader: Some(self::process_with_path(
                9400,
                "codex-aarch64-apple-darwin",
                "/opt/homebrew/Caskroom/codex/0.137.0/codex-aarch64-apple-darwin",
            )),
            shell_pid: Some(9322),
        });

        assert2::assert!(let PaneCmdObservation::FgCmd { cmd } = observation);
        pretty_assertions::assert_eq!(cmd.executable, "codex-aarch64-apple-darwin");
        pretty_assertions::assert_eq!(cmd.pid, 9400);
        pretty_assertions::assert_eq!(agent_for_cmd(&cmd), Some(Agent::Codex));
    }

    #[test]
    fn test_observe_pane_cmd_when_leader_is_missing_returns_unknown() {
        let observation = observe_pane_cmd(&PaneCmdSnapshot {
            fg_process_group: Some(9400),
            fg_process_group_leader: None,
            shell_pid: Some(9322),
        });

        pretty_assertions::assert_eq!(
            observation,
            PaneCmdObservation::Unknown {
                reason: PaneCmdUnknownReason::MissingFgProcess,
            },
        );
    }

    #[test]
    fn test_observe_pane_cmd_when_cmd_exits_and_shell_is_fg_returns_shell() {
        let observation = observe_pane_cmd(&PaneCmdSnapshot {
            fg_process_group: Some(9322),
            fg_process_group_leader: Some(self::process(9322, "zsh")),
            shell_pid: Some(9322),
        });

        pretty_assertions::assert_eq!(observation, PaneCmdObservation::Shell);
    }

    #[test]
    fn test_observe_pane_cmd_when_fg_is_non_agent_returns_cmd_without_agent() {
        let observation = observe_pane_cmd(&PaneCmdSnapshot {
            fg_process_group: Some(4242),
            fg_process_group_leader: Some(self::process(4242, "nvim")),
            shell_pid: Some(9322),
        });

        assert2::assert!(let PaneCmdObservation::FgCmd { cmd } = observation);
        pretty_assertions::assert_eq!(cmd.executable, "nvim");
        pretty_assertions::assert_eq!(agent_for_cmd(&cmd), None);
    }

    #[rstest]
    #[case::claude("claude", Some(Agent::Claude))]
    #[case::claude_code("claude-code", Some(Agent::Claude))]
    #[case::codex("codex", Some(Agent::Codex))]
    #[case::codex_cask("codex-aarch64-apple-darwin", Some(Agent::Codex))]
    #[case::cursor("cursor", Some(Agent::Cursor))]
    #[case::cursor_agent("cursor-agent", Some(Agent::Cursor))]
    #[case::gemini("gemini", Some(Agent::Gemini))]
    #[case::opencode("opencode", Some(Agent::Opencode))]
    #[case::rg_codex("rg-codex", None)]
    #[case::notcodex("notcodex", None)]
    #[case::rg("rg", None)]
    fn test_agent_for_cmd_when_executable_varies_returns_direct_agent_only(
        #[case] executable: &str,
        #[case] expected: Option<Agent>,
    ) {
        let cmd = PaneCmd {
            executable: executable.to_owned(),
            path: None,
            pid: 1,
        };

        pretty_assertions::assert_eq!(agent_for_cmd(&cmd), expected);
    }

    #[test]
    fn test_agent_for_cmd_when_cursor_agent_execs_bundled_node_returns_cursor() {
        let cmd = PaneCmd {
            executable: "node".to_owned(),
            path: Some("/Users/me/.local/share/cursor-agent/versions/2026.06.04-5fd875e/node".to_owned()),
            pid: 1,
        };

        pretty_assertions::assert_eq!(agent_for_cmd(&cmd), Some(Agent::Cursor));
    }

    #[test]
    fn test_agent_for_cmd_when_plain_node_runs_returns_no_agent() {
        let cmd = PaneCmd {
            executable: "node".to_owned(),
            path: Some("/opt/homebrew/bin/node".to_owned()),
            pid: 1,
        };

        pretty_assertions::assert_eq!(agent_for_cmd(&cmd), None);
    }

    #[test]
    fn test_agent_for_cmd_when_claude_path_is_versioned_runtime_returns_claude() {
        let cmd = PaneCmd {
            executable: "2.1.165".to_owned(),
            path: Some("/Users/me/.local/share/claude/versions/2.1.165".to_owned()),
            pid: 1,
        };

        pretty_assertions::assert_eq!(agent_for_cmd(&cmd), Some(Agent::Claude));
    }

    fn process(pid: u32, name: &str) -> PaneProcess {
        PaneProcess {
            name: Some(name.to_owned()),
            path: None,
            pid,
        }
    }

    fn process_with_path(pid: u32, name: &str, path: &str) -> PaneProcess {
        PaneProcess {
            name: Some(name.to_owned()),
            path: Some(path.to_owned()),
            pid,
        }
    }
}
