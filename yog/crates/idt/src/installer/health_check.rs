//! Health-check execution and outcomes.

use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use rootcause::Report;
use ytil_cmd::Cmd;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt;

use crate::installer::Installer;

/// Runs a command used to verify an installed binary.
pub fn run_cmd(mut cmd: Command) -> rootcause::Result<String> {
    let output = cmd.exec()?;
    let output = std::str::from_utf8(&output.stdout).map_err(|err| CmdError::Utf8 {
        cmd: Cmd::from(&cmd),
        source: err,
    })?;
    Ok(output.to_owned())
}

pub(super) enum HealthCheckOutcome {
    NotRun,
    Unavailable,
    Passed { duration: Duration, output: String },
    Failed { duration: Duration, error: Report },
}

impl HealthCheckOutcome {
    pub(super) const fn duration(&self) -> Option<Duration> {
        match self {
            Self::Passed { duration, .. } | Self::Failed { duration, .. } => Some(*duration),
            Self::NotRun | Self::Unavailable => None,
        }
    }

    pub(super) const fn status(&self) -> &'static str {
        match self {
            Self::NotRun => "not-run",
            Self::Unavailable => "unavailable",
            Self::Passed { .. } => "passed",
            Self::Failed { .. } => "failed",
        }
    }
}

/// Runs an available health check and records its result and duration.
pub(super) fn run(installer: &(impl Installer + ?Sized)) -> HealthCheckOutcome {
    let start = Instant::now();
    let Some(result) = installer.health_check() else {
        return HealthCheckOutcome::Unavailable;
    };
    let duration = start.elapsed();

    match result {
        Ok(output) => HealthCheckOutcome::Passed { duration, output },
        Err(error) => HealthCheckOutcome::Failed { duration, error },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::TestInstaller;

    #[rstest::rstest]
    #[case::passed(Some(Ok("version output".to_owned())), "passed")]
    #[case::failed(Some(Err(rootcause::report!("health check failed"))), "failed")]
    #[case::unavailable(None, "unavailable")]
    fn test_heatlh_check_run_when_invoked_preserves_result_and_records_duration(
        #[case] result: Option<rootcause::Result<String>>,
        #[case] expected_status: &str,
    ) {
        // Reports cannot be compared directly; compare their complete rendered error.
        let expected_result = result
            .as_ref()
            .map(|result| result.as_ref().cloned().map_err(ToString::to_string));
        let result = std::sync::Mutex::new(result);
        let installer = TestInstaller {
            install: || panic!("checking health must not install the tool"),
            health_check: || result.lock().unwrap().take(),
        };
        let start = Instant::now();

        let outcome = run(&installer);

        assert_eq!(outcome.status(), expected_status);
        assert_eq!(outcome.duration().is_some(), expected_result.is_some());
        if let Some(duration) = outcome.duration() {
            assert!(duration <= start.elapsed());
        }
        let actual_result = match outcome {
            HealthCheckOutcome::Passed { output, .. } => Some(Ok(output)),
            HealthCheckOutcome::Failed { error, .. } => Some(Err(error.to_string())),
            HealthCheckOutcome::Unavailable => None,
            HealthCheckOutcome::NotRun => panic!("running a health check must resolve its availability"),
        };
        assert_eq!(actual_result, expected_result);
    }
}
