//! Installation reports, diagnostics, and output formatting.

use std::time::Duration;

use owo_colors::AnsiColors;
use owo_colors::OwoColorize;
use rootcause::Report;

use crate::downloaders::checksum::ChecksumOutcome;
use crate::installer::health_check::HealthCheckOutcome;

/// The complete result used for both diagnostics and the tool name's color.
/// Failed installations cannot contain a health-check result.
pub(super) enum InstallationReport {
    Installed {
        artifact_duration: Duration,
        checksum: ChecksumOutcome,
        health_check: HealthCheckOutcome,
        total_duration: Duration,
    },
    InstallFailed {
        install_duration: Duration,
        error: Report,
        total_duration: Duration,
    },
}

impl InstallationReport {
    pub(super) fn print(self, bin_name: &str) -> rootcause::Result<()> {
        let color = self.color();
        let name = bin_name.color(color).bold().to_string();
        let timing = self.timing();
        let check_statuses = self.check_statuses();

        match self {
            Self::InstallFailed { error, .. } => {
                eprintln!(
                    "{name} error installing {check_statuses}\n{}",
                    format!("{error:#?}").red()
                );
                Err(error)
            }
            Self::Installed { health_check, .. } => match health_check {
                HealthCheckOutcome::Passed { output, .. } => {
                    println!(
                        "{name} {timing} {check_statuses} health_check_output=\n{}",
                        output.trim_matches(|c| c == '\n' || c == '\r')
                    );
                    Ok(())
                }
                HealthCheckOutcome::Failed { error, .. } => {
                    eprintln!(
                        "{} error in health check {timing} {check_statuses}\n{}",
                        bin_name.color(color),
                        format!("{error:#?}").red()
                    );
                    Err(error)
                }
                HealthCheckOutcome::NotRun | HealthCheckOutcome::Unavailable => {
                    println!("{name} {timing} {check_statuses}");
                    Ok(())
                }
            },
        }
    }

    fn check_statuses(&self) -> String {
        let checksum_status = match self {
            Self::Installed { checksum, .. } => checksum.status(),
            Self::InstallFailed { .. } => "unknown",
        };

        format!(
            "checksum={checksum_status} health_check={}",
            self.health_check().status()
        )
    }

    const fn health_check(&self) -> &HealthCheckOutcome {
        match self {
            Self::Installed { health_check, .. } => health_check,
            Self::InstallFailed { .. } => &HealthCheckOutcome::NotRun,
        }
    }

    const fn total_duration(&self) -> Duration {
        match self {
            Self::Installed { total_duration, .. } | Self::InstallFailed { total_duration, .. } => *total_duration,
        }
    }

    const fn color(&self) -> AnsiColors {
        match self {
            Self::InstallFailed { .. }
            | Self::Installed {
                health_check: HealthCheckOutcome::Failed { .. },
                ..
            } => AnsiColors::Red,
            Self::Installed {
                checksum: ChecksumOutcome::Unavailable,
                ..
            } => AnsiColors::Blue,
            Self::Installed {
                checksum: ChecksumOutcome::Passed { .. } | ChecksumOutcome::NotNeeded,
                ..
            } => AnsiColors::Green,
        }
    }

    fn timing(&self) -> String {
        // Successful installation time includes checksum verification.
        let install_duration = match self {
            Self::Installed {
                artifact_duration,
                checksum,
                ..
            } => artifact_duration.saturating_add(checksum.duration().unwrap_or_default()),
            Self::InstallFailed { install_duration, .. } => *install_duration,
        };

        format!(
            "install_time={install_duration:?} health_check_time={:?} total_time={:?}",
            self.health_check().duration(),
            self.total_duration()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::not_needed(ChecksumOutcome::NotNeeded, AnsiColors::Green, "2s", "not-needed")]
    #[case::unavailable(ChecksumOutcome::Unavailable, AnsiColors::Blue, "2s", "unavailable")]
    #[case::passed(ChecksumOutcome::Passed { duration: Duration::from_millis(4) }, AnsiColors::Green, "2.004s", "passed")]
    fn test_installation_report_when_installed_uses_checksum_color_unless_health_fails(
        #[case] checksum: ChecksumOutcome,
        #[case] success_color: AnsiColors,
        #[case] expected_install_time: &str,
        #[case] expected_checksum_status: &str,
        #[values("passed", "failed", "unavailable")] status: &str,
    ) {
        let duration = Duration::from_millis(3);
        let health_check = match status {
            "passed" => HealthCheckOutcome::Passed {
                duration,
                output: "version output".to_owned(),
            },
            "failed" => HealthCheckOutcome::Failed {
                duration,
                error: rootcause::report!("health check failed"),
            },
            _ => HealthCheckOutcome::Unavailable,
        };
        let report = InstallationReport::Installed {
            artifact_duration: Duration::from_secs(2),
            checksum,
            health_check,
            total_duration: Duration::from_secs(3),
        };
        let expected_color = if status == "failed" {
            AnsiColors::Red
        } else {
            success_color
        };
        let expected_health_time = if status == "unavailable" { "None" } else { "Some(3ms)" };

        let actual = (report.color(), report.check_statuses(), report.timing());

        assert_eq!(
            actual,
            (
                expected_color,
                format!("checksum={expected_checksum_status} health_check={status}"),
                format!("install_time={expected_install_time} health_check_time={expected_health_time} total_time=3s")
            )
        );
    }

    #[test]
    fn test_installation_report_when_installation_fails_uses_red_and_marks_health_not_run() {
        let error: rootcause::Report = rootcause::report!("installation failed");
        let expected_error = error.to_string();
        let report = InstallationReport::InstallFailed {
            install_duration: Duration::from_secs(2),
            error,
            total_duration: Duration::from_secs(3),
        };

        let actual = (
            report.color(),
            report.check_statuses(),
            report.health_check().duration(),
        );

        assert_eq!(
            actual,
            (
                AnsiColors::Red,
                "checksum=unknown health_check=not-run".to_owned(),
                None
            )
        );
        assert_eq!(report.print("tool").unwrap_err().to_string(), expected_error);
    }
}
