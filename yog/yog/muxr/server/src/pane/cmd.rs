use std::path::Path;

use libproc::processes::ProcFilter;

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
        let shell_pid = handle.process_id();
        let fg_process_group = handle.fg_process_group();
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
    FgCmd(FgCmd),
    Shell,
    Unknown { reason: PaneCmdUnknownReason },
}

/// Fg command observation with a lazy same-process-group fallback.
///
/// Wrappers such as `ags` can stay the fg group leader while a configured agent is a child process. Consumers
/// check the leader first and only scan the process group when the leader itself is not tracked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FgCmd {
    leader_cmd: Option<PaneCmd>,
    process_group: FgProcessGroup,
    process_group_cmds: Option<Result<Vec<PaneCmd>, ProcessGroupLookupError>>,
}

impl FgCmd {
    const fn live(leader_cmd: Option<PaneCmd>, process_group: u32, shell_pid: Option<u32>) -> Self {
        Self {
            leader_cmd,
            process_group: FgProcessGroup {
                pid: process_group,
                shell_pid,
            },
            process_group_cmds: None,
        }
    }

    pub const fn leader_cmd(&self) -> Option<&PaneCmd> {
        self.leader_cmd.as_ref()
    }

    pub fn process_group_cmds(&self) -> Result<Vec<PaneCmd>, ProcessGroupLookupError> {
        if let Some(process_group_cmds) = &self.process_group_cmds {
            return process_group_cmds.clone();
        }
        self.process_group.commands()
    }

    #[cfg(test)]
    pub const fn from_test_cmd(cmd: PaneCmd) -> Self {
        Self::from_test_group(Some(cmd), Ok(Vec::new()))
    }

    #[cfg(test)]
    pub const fn from_test_group(
        leader_cmd: Option<PaneCmd>,
        process_group_cmds: Result<Vec<PaneCmd>, ProcessGroupLookupError>,
    ) -> Self {
        Self {
            leader_cmd,
            process_group: FgProcessGroup {
                pid: 0,
                shell_pid: None,
            },
            process_group_cmds: Some(process_group_cmds),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FgProcessGroup {
    pid: u32,
    shell_pid: Option<u32>,
}

impl FgProcessGroup {
    fn commands(&self) -> Result<Vec<PaneCmd>, ProcessGroupLookupError> {
        self::process_group_members(self.pid).map(|members| {
            members
                .into_iter()
                .filter(|member| self.shell_pid != Some(member.pid))
                .filter_map(|member| member.cmd())
                .collect()
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessGroupLookupError {
    Failed,
}

impl From<&PaneCmdSnapshot> for PaneCmdObservation {
    fn from(snapshot: &PaneCmdSnapshot) -> Self {
        let Some(fg_process_group) = snapshot.fg_process_group else {
            return Self::Unknown {
                reason: PaneCmdUnknownReason::MissingFgProcessGroup,
            };
        };
        if snapshot
            .shell_pid
            .is_some_and(|shell_pid| fg_process_group == shell_pid)
        {
            return Self::Shell;
        }

        if let Some(process) = &snapshot.fg_process_group_leader {
            if snapshot.shell_pid.is_some_and(|shell_pid| process.pid == shell_pid) {
                return Self::Shell;
            }
            return Self::FgCmd(FgCmd::live(process.cmd(), fg_process_group, snapshot.shell_pid));
        }

        Self::FgCmd(FgCmd::live(None, fg_process_group, snapshot.shell_pid))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaneCmdUnknownReason {
    MissingFgProcessGroup,
}

fn process_executable(raw: Option<&str>) -> Option<&str> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }

    Path::new(raw).file_name().and_then(|name| name.to_str()).or(Some(raw))
}

fn process_group_members(process_group: u32) -> Result<Vec<PaneProcess>, ProcessGroupLookupError> {
    let Ok(mut pids) = libproc::processes::pids_by_type(ProcFilter::ByProgramGroup { pgrpid: process_group }) else {
        return Err(ProcessGroupLookupError::Failed);
    };
    pids.sort_unstable();
    Ok(pids.into_iter().filter_map(PaneProcess::from_pid).collect())
}

#[cfg(feature = "benchmarking")]
pub fn benchmark_current_process_observation() -> (bool, bool) {
    let observation = PaneProcess::from_pid(std::process::id());
    (
        observation.as_ref().and_then(|process| process.name.as_ref()).is_some(),
        observation.as_ref().and_then(|process| process.path.as_ref()).is_some(),
    )
}

#[cfg(test)]
mod tests {
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_observe_pane_cmd_when_fg_leader_is_shell_returns_shell() {
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
            fg_process_group: Some(9322),
            fg_process_group_leader: Some(self::process(9322, "zsh")),
            shell_pid: Some(9322),
        });

        assert_that!(observation, eq(PaneCmdObservation::Shell));
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

        assert_that!(observation, matches_pattern!(PaneCmdObservation::FgCmd(anything())));
        let fg_cmd = match observation {
            PaneCmdObservation::FgCmd(fg_cmd) => fg_cmd,
            PaneCmdObservation::Shell | PaneCmdObservation::Unknown { .. } => {
                panic!("asserted foreground command")
            }
        };
        let cmd = fg_cmd.leader_cmd().expect("expected leader command");
        assert_that!(cmd.executable, eq("demo-aarch64-apple-darwin"));
        assert_that!(cmd.pid, eq(9400));
    }

    #[test]
    fn test_observe_pane_cmd_when_leader_is_missing_returns_fg_cmd_without_leader() {
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
            fg_process_group: Some(9400),
            fg_process_group_leader: None,
            shell_pid: Some(9322),
        });

        assert_that!(observation, matches_pattern!(PaneCmdObservation::FgCmd(anything())));
        let fg_cmd = match observation {
            PaneCmdObservation::FgCmd(fg_cmd) => fg_cmd,
            PaneCmdObservation::Shell | PaneCmdObservation::Unknown { .. } => {
                panic!("asserted foreground command")
            }
        };
        assert_that!(fg_cmd.leader_cmd(), none());
    }

    #[test]
    fn test_observe_pane_cmd_when_fg_group_has_child_command_returns_group_cmd() {
        let observation = PaneCmdObservation::FgCmd(FgCmd::from_test_group(
            Some(self::cmd(17869, "ags")),
            Ok(vec![self::cmd(17989, "codex")]),
        ));

        assert_that!(observation, matches_pattern!(PaneCmdObservation::FgCmd(anything())));
        let fg_cmd = match observation {
            PaneCmdObservation::FgCmd(fg_cmd) => fg_cmd,
            PaneCmdObservation::Shell | PaneCmdObservation::Unknown { .. } => {
                panic!("asserted foreground command")
            }
        };
        assert_that!(
            fg_cmd.leader_cmd().expect("expected leader command").executable,
            eq("ags")
        );
        assert_that!(fg_cmd.process_group_cmds(), eq(Ok(vec![self::cmd(17989, "codex")])));
    }

    #[test]
    fn test_observe_pane_cmd_when_cmd_exits_and_shell_is_fg_returns_shell() {
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
            fg_process_group: Some(9322),
            fg_process_group_leader: Some(self::process(9322, "zsh")),
            shell_pid: Some(9322),
        });

        assert_that!(observation, eq(PaneCmdObservation::Shell));
    }

    #[test]
    fn test_observe_pane_cmd_when_fg_is_untracked_returns_cmd() {
        let observation = PaneCmdObservation::from(&PaneCmdSnapshot {
            fg_process_group: Some(4242),
            fg_process_group_leader: Some(self::process(4242, "nvim")),
            shell_pid: Some(9322),
        });

        assert_that!(observation, matches_pattern!(PaneCmdObservation::FgCmd(anything())));
        let fg_cmd = match observation {
            PaneCmdObservation::FgCmd(fg_cmd) => fg_cmd,
            PaneCmdObservation::Shell | PaneCmdObservation::Unknown { .. } => {
                panic!("asserted foreground command")
            }
        };
        assert_that!(
            fg_cmd.leader_cmd().expect("expected leader command").executable,
            eq("nvim")
        );
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

    fn cmd(pid: u32, executable: &str) -> PaneCmd {
        PaneCmd {
            executable: executable.to_owned(),
            path: None,
            pid,
        }
    }
}
