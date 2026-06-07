use std::path::Path;

use crate::pty::PtyHandle;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneProcess {
    pub name: Option<String>,
    pub path: Option<String>,
    pub pid: u32,
}

impl PaneProcess {
    fn from_pid(pid: u32) -> Option<Self> {
        let Ok(pid_i32) = i32::try_from(pid) else {
            return None;
        };
        Some(Self {
            // Process lookup can race with foreground-process exit. Treat misses as absent metadata; observation stays
            // Unknown instead of clearing a live command from one failed sample.
            name: libproc::proc_pid::name(pid_i32).ok(),
            path: libproc::proc_pid::pidpath(pid_i32).ok(),
            pid,
        })
    }

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

impl TryFrom<&PtyHandle> for PaneCmdSnapshot {
    type Error = rootcause::Report;

    fn try_from(handle: &PtyHandle) -> rootcause::Result<Self> {
        let shell_pid = handle.process_id()?;
        let fg_process_group = handle.fg_process_group()?;
        let fg_process_group_leader = fg_process_group.and_then(PaneProcess::from_pid);

        Ok(Self {
            fg_process_group,
            fg_process_group_leader,
            shell_pid,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaneCmdObservation {
    FgCmd { cmd: PaneCmd },
    Shell,
    Unknown { reason: PaneCmdUnknownReason },
}

impl From<&PaneCmdSnapshot> for PaneCmdObservation {
    fn from(snapshot: &PaneCmdSnapshot) -> Self {
        if snapshot.fg_process_group.is_none() {
            return Self::Unknown {
                reason: PaneCmdUnknownReason::MissingFgProcessGroup,
            };
        }

        if let Some(process) = &snapshot.fg_process_group_leader {
            return self::observe_process(process, snapshot.shell_pid);
        }

        Self::Unknown {
            reason: PaneCmdUnknownReason::MissingFgProcess,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneCmdUnknownReason {
    FgProcessHasNoExecutable,
    MissingFgProcess,
    MissingFgProcessGroup,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observe_pane_cmd_when_fg_leader_is_shell_returns_shell() {
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
            fg_process_group: Some(9322),
            fg_process_group_leader: Some(self::process(9322, "zsh")),
            shell_pid: Some(9322),
        });

        pretty_assertions::assert_eq!(observation, PaneCmdObservation::Shell);
    }

    #[test]
    fn test_observe_pane_cmd_when_fg_leader_has_path_returns_fg_cmd() {
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
            fg_process_group: Some(9400),
            fg_process_group_leader: Some(self::process_with_path(
                9400,
                "demo-aarch64-apple-darwin",
                "/opt/homebrew/Caskroom/demo/0.137.0/demo-aarch64-apple-darwin",
            )),
            shell_pid: Some(9322),
        });

        assert2::assert!(let PaneCmdObservation::FgCmd { cmd } = observation);
        pretty_assertions::assert_eq!(cmd.executable, "demo-aarch64-apple-darwin");
        pretty_assertions::assert_eq!(cmd.pid, 9400);
    }

    #[test]
    fn test_observe_pane_cmd_when_leader_is_missing_returns_unknown() {
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
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
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
            fg_process_group: Some(9322),
            fg_process_group_leader: Some(self::process(9322, "zsh")),
            shell_pid: Some(9322),
        });

        pretty_assertions::assert_eq!(observation, PaneCmdObservation::Shell);
    }

    #[test]
    fn test_observe_pane_cmd_when_fg_is_untracked_returns_cmd() {
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
            fg_process_group: Some(4242),
            fg_process_group_leader: Some(self::process(4242, "nvim")),
            shell_pid: Some(9322),
        });

        assert2::assert!(let PaneCmdObservation::FgCmd { cmd } = observation);
        pretty_assertions::assert_eq!(cmd.executable, "nvim");
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
