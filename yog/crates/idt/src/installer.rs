//! Shared installer contracts and installation orchestration.

use std::process::Command;
use std::time::Instant;

use crate::downloaders::checksum::ChecksumOutcome;
use crate::downloaders::http::DownloadedArtifact;
use crate::installer::health_check::run;
use crate::installer::health_check::run_cmd;
use crate::installer::report::InstallationReport;

pub mod health_check;
mod report;

/// Trait for installing development tools.
pub trait Installer: Sync + Send {
    /// Returns the binary name.
    fn bin_name(&self) -> &'static str;

    /// Installs the tool.
    fn install(&self) -> rootcause::Result<InstallOutcome>;

    /// Runs the installed binary to verify it is functional.
    fn health_check(&self) -> Option<rootcause::Result<String>> {
        let args = self.health_check_args()?;
        let mut cmd = Command::new(self.bin_name());
        cmd.args(args);

        Some(run_cmd(cmd))
    }

    /// Execute install + optional health check with timing output.
    ///
    /// # Errors
    /// - Install or health check phase fails.
    fn run(&self) -> rootcause::Result<()> {
        let start = Instant::now();
        let result = self.install();
        let install_duration = start.elapsed();

        let report = match result {
            Ok(outcome) => {
                let checksum = outcome.checksum();
                let artifact_duration = install_duration.saturating_sub(checksum.duration().unwrap_or_default());
                let health_check = run(self);

                InstallationReport::Installed {
                    artifact_duration,
                    checksum,
                    health_check,
                    total_duration: start.elapsed(),
                }
            }
            Err(error) => InstallationReport::InstallFailed {
                install_duration,
                error,
                total_duration: start.elapsed(),
            },
        };

        report.print(self.bin_name())
    }

    /// Returns arguments for the health check (e.g. `--version`).
    fn health_check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }
}

pub trait SystemDependent {
    fn target_arch_and_os(&self) -> (&str, &str);
}

/// Successful installation, including checksum evidence for a downloaded artifact.
pub enum InstallOutcome {
    ChecksumNotNeeded,
    Downloaded(DownloadedArtifact),
}

impl InstallOutcome {
    const fn checksum(&self) -> ChecksumOutcome {
        match self {
            Self::ChecksumNotNeeded => ChecksumOutcome::NotNeeded,
            Self::Downloaded(artifact) => artifact.checksum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufRead;
    use std::io::BufReader;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use rootcause::prelude::ResultExt;

    use super::*;
    use crate::downloaders::http;
    use crate::downloaders::http::deflate::ChecksumSource;
    use crate::downloaders::http::deflate::HttpDeflateOption;

    pub(super) struct TestInstaller<I, H> {
        pub(super) install: I,
        pub(super) health_check: H,
    }

    impl<I, H> Installer for TestInstaller<I, H>
    where
        I: Fn() -> rootcause::Result<InstallOutcome> + Send + Sync,
        H: Fn() -> Option<rootcause::Result<String>> + Send + Sync,
    {
        fn bin_name(&self) -> &'static str {
            "test-tool"
        }

        fn install(&self) -> rootcause::Result<InstallOutcome> {
            (self.install)()
        }

        fn health_check(&self) -> Option<rootcause::Result<String>> {
            (self.health_check)()
        }
    }

    #[test]
    fn test_installer_run_when_installation_fails_skips_health_check() {
        let installer = TestInstaller {
            install: || Err(rootcause::report!("build failed")),
            health_check: || panic!("health check must not run after failed installation"),
        };

        let result = installer.run();

        let error = result.unwrap_err();
        assert!(error.to_string().contains("build failed"));
    }

    #[rstest::rstest]
    #[case::verified(Some("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"))]
    #[case::unverified(None)]
    fn test_installer_run_when_artifact_is_downloaded_checks_integrity_and_health(
        #[case] checksum: Option<&'static str>,
        #[values("passed", "failed", "unavailable")] health_status: &str,
    ) {
        let mut bodies = vec!["hello world"];
        bodies.extend(checksum);
        let (url, server) = serve_download(bodies);
        let checksums_url = format!("{url}/checksums");
        let source = checksum.map(|_| ChecksumSource {
            checksums_url: &checksums_url,
            filename: "tool",
        });
        let dir = tempfile::tempdir().unwrap();
        let dest_path = dir.path().join("tool");
        let observed_checksum = std::sync::Mutex::new(None);
        let installer = TestInstaller {
            install: || {
                let download = http::run(
                    &url,
                    &HttpDeflateOption::WriteTo { dest_path: &dest_path },
                    source.as_ref(),
                )?;
                *observed_checksum.lock().unwrap() = Some(download.checksum());
                Ok(InstallOutcome::Downloaded(download))
            },
            health_check: || match health_status {
                "passed" => Some(Ok("version".to_owned())),
                "failed" => Some(Err(rootcause::report!("health check failed"))),
                _ => None,
            },
        };

        let start = Instant::now();

        let result = installer.run();

        server.join().unwrap();
        match (
            observed_checksum
                .lock()
                .unwrap()
                .expect("download must produce checksum evidence"),
            checksum,
        ) {
            (ChecksumOutcome::Passed { duration }, Some(_)) => assert!(duration <= start.elapsed()),
            (ChecksumOutcome::Unavailable, None) => {}
            (outcome, _) => panic!("unexpected checksum outcome: {outcome:?}"),
        }
        assert_eq!(std::fs::read(&dest_path).unwrap(), b"hello world");
        if health_status == "failed" {
            let error = result.unwrap_err();
            assert!(error.to_string().contains("health check failed"));
        } else {
            result.unwrap();
        }
    }

    #[rstest::rstest]
    #[case::mismatch(
        "0000000000000000000000000000000000000000000000000000000000000000",
        "error checksum mismatch"
    )]
    #[case::missing_entry("abc  another-tool", "error checksum entry not found")]
    fn test_installer_run_when_checksum_fails_rejects_artifact_and_skips_health_check(
        #[case] checksum: &'static str,
        #[case] expected_error: &str,
    ) {
        let (url, server) = serve_download(vec!["hello world", checksum]);
        let checksums_url = format!("{url}/checksums");
        let source = ChecksumSource {
            checksums_url: &checksums_url,
            filename: "tool",
        };
        let dir = tempfile::tempdir().unwrap();
        let dest_path = dir.path().join("tool");
        let installer = TestInstaller {
            install: || {
                let download = http::run(
                    &url,
                    &HttpDeflateOption::WriteTo { dest_path: &dest_path },
                    Some(&source),
                )?;
                Ok(InstallOutcome::Downloaded(download))
            },
            health_check: || panic!("health check must not run after checksum failure"),
        };

        let result = installer.run();

        server.join().unwrap();
        let error = result.unwrap_err();
        assert!(!dest_path.exists());
        assert!(error.to_string().contains(expected_error));
    }

    #[test]
    fn test_installer_run_when_placement_fails_returns_original_error() {
        let (url, server) = serve_download(vec![
            "hello world",
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        ]);
        let checksums_url = format!("{url}/checksums");
        let source = ChecksumSource {
            checksums_url: &checksums_url,
            filename: "tool",
        };
        let dir = tempfile::tempdir().unwrap();
        // The parent directory does not exist, so final placement fails after verification.
        let dest_path = dir.path().join("missing").join("tool");
        let installer = TestInstaller {
            install: || {
                let download = http::run(
                    &url,
                    &HttpDeflateOption::WriteTo { dest_path: &dest_path },
                    Some(&source),
                )?;
                Ok(InstallOutcome::Downloaded(download))
            },
            health_check: || panic!("health check must not run after placement failure"),
        };

        let result = installer.run();

        server.join().unwrap();
        let error = result.unwrap_err();
        assert!(!dest_path.exists());
        assert!(error.to_string().contains("error copying tmp file to dest"));
    }

    #[rstest::rstest]
    #[case::verified(Some("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"))]
    #[case::unverified(None)]
    fn test_installer_run_when_finalization_fails_returns_original_error(#[case] checksum: Option<&'static str>) {
        let mut bodies = vec!["hello world"];
        bodies.extend(checksum);
        let (url, server) = serve_download(bodies);
        let checksums_url = format!("{url}/checksums");
        let source = checksum.map(|_| ChecksumSource {
            checksums_url: &checksums_url,
            filename: "tool",
        });
        let dir = tempfile::tempdir().unwrap();
        let dest_path = dir.path().join("tool");
        let installer = TestInstaller {
            install: || {
                let download = http::run(
                    &url,
                    &HttpDeflateOption::WriteTo { dest_path: &dest_path },
                    source.as_ref(),
                )?;
                std::fs::rename(&download.path, dir.path().join("missing").join("tool"))
                    .context("error finalizing binary")?;
                Ok(InstallOutcome::Downloaded(download))
            },
            health_check: || panic!("health check must not run after finalization fails"),
        };

        let result = installer.run();

        server.join().unwrap();
        let error = result.unwrap_err();
        assert_eq!(std::fs::read(&dest_path).unwrap(), b"hello world");
        assert!(error.to_string().contains("error finalizing binary"));
    }

    #[rstest::rstest]
    #[case::artifact(vec![("404 Not Found", "")], "error downloading")]
    #[case::checksum(vec![("200 OK", "hello world"), ("404 Not Found", "")], "error downloading checksums file")]
    fn test_installer_run_when_http_fails_returns_error_and_skips_health_check(
        #[case] responses: Vec<(&'static str, &'static str)>,
        #[case] expected_error: &str,
    ) {
        let (url, server) = serve_responses(responses);
        let checksums_url = format!("{url}/checksums");
        let source = ChecksumSource {
            checksums_url: &checksums_url,
            filename: "tool",
        };
        let dir = tempfile::tempdir().unwrap();
        let dest_path = dir.path().join("tool");
        let installer = TestInstaller {
            install: || {
                let download = http::run(
                    &url,
                    &HttpDeflateOption::WriteTo { dest_path: &dest_path },
                    Some(&source),
                )?;
                Ok(InstallOutcome::Downloaded(download))
            },
            health_check: || panic!("health check must not run after an HTTP failure"),
        };

        let result = installer.run();

        server.join().unwrap();
        let error = result.unwrap_err();
        assert!(!dest_path.exists());
        assert!(error.to_string().contains(expected_error));
    }

    fn serve_download(bodies: Vec<&'static str>) -> (String, JoinHandle<()>) {
        serve_responses(bodies.into_iter().map(|body| ("200 OK", body)).collect())
    }

    fn serve_responses(responses: Vec<(&'static str, &'static str)>) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                loop {
                    line.clear();
                    let size = reader.read_line(&mut line).unwrap();
                    if size == 0 || line == "\r\n" {
                        break;
                    }
                }

                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });

        (url, server)
    }
}
