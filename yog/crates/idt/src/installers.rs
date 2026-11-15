use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use owo_colors::AnsiColors;
use owo_colors::OwoColorize;
use rootcause::bail;
use rootcause::prelude::ResultExt;
use ytil_cmd::Cmd;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt;

use crate::downloaders::http::DownloadedArtifact;

pub mod alacritty;
pub mod bash_language_server;
pub mod cargo;
pub mod commitlint;
pub mod deno;
pub mod docker_langserver;
pub mod eslint_d;
pub mod graphql_lsp;
pub mod hadolint;
pub mod helm_ls;
pub mod lua_ls;
pub mod marksman;
pub mod nvim;
pub mod opencode;
pub mod prettierd;
pub mod quicktype;
pub mod rio;
pub mod ruff_lsp;
pub mod shellcheck;
pub mod sql_language_server;
pub mod starship;
pub mod terraform_ls;
pub mod typescript_language_server;
pub mod vscode_langservers;
pub mod yaml_language_server;
pub mod zellij;

/// Describes whether installation involved a directly downloaded artifact.
pub enum InstallOutcome {
    NoArtifact,
    Downloaded(DownloadedArtifact),
}

impl InstallOutcome {
    fn success_bin_name(&self, bin_name: &str, unverified_color: AnsiColors) -> String {
        let color = match self {
            Self::NoArtifact => AnsiColors::Green,
            Self::Downloaded(artifact) if artifact.checksum_verified() => AnsiColors::Green,
            Self::Downloaded(_) => unverified_color,
        };

        bin_name.color(color).bold().to_string()
    }
}

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

        Some(run_health_check(cmd))
    }

    /// Execute install + optional health check with timing output.
    ///
    /// # Errors
    /// - Install or health check phase fails.
    fn run(&self) -> rootcause::Result<()> {
        let start = Instant::now();

        // Install phase
        let outcome = self.install().inspect_err(|err| {
            eprintln!(
                "{} error installing\n{}",
                self.bin_name().red().bold(),
                format!("{err:#?}").red()
            );
        })?;

        let past_install = Instant::now();

        // Health check phase (optional)
        let mut health_check_duration = None;
        let health_check_start = Instant::now();
        let health_check_res = self.health_check();
        if health_check_res.is_some() {
            health_check_duration = Some(health_check_start.elapsed());
        }

        match health_check_res {
            Some(Ok(health_check_output)) => {
                let styled_bin_name = outcome.success_bin_name(self.bin_name(), AnsiColors::Yellow);
                println!(
                    "{styled_bin_name} {} health_check_output=\n{}",
                    format_timing(start, past_install, health_check_duration),
                    health_check_output.trim_matches(|c| c == '\n' || c == '\r')
                );
            }
            Some(Err(err)) => {
                eprintln!(
                    "{} error in health check {}\n{}",
                    self.bin_name().red(),
                    format_timing(start, past_install, health_check_duration),
                    format!("{err:#?}").red()
                );
                return Err(err);
            }
            None => {
                let styled_bin_name = outcome.success_bin_name(self.bin_name(), AnsiColors::Magenta);
                println!(
                    "{styled_bin_name} {}",
                    format_timing(start, past_install, health_check_duration),
                );
            }
        }

        Ok(())
    }

    /// Returns arguments for the health check (e.g. `--version`).
    fn health_check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }
}

/// Runs a command used to verify an installed binary.
pub fn run_health_check(mut cmd: Command) -> rootcause::Result<String> {
    let output = cmd.exec()?;
    let output = std::str::from_utf8(&output.stdout).map_err(|err| CmdError::Utf8 {
        cmd: Cmd::from(&cmd),
        source: err,
    })?;
    Ok(output.to_owned())
}

pub trait SystemDependent {
    fn target_arch_and_os(&self) -> (&str, &str);
}

/// Common install pattern for npm-based tools: download via npm, symlink the binary, and make it executable.
///
/// # Errors
/// - If npm download fails.
/// - If symlink creation fails.
/// - If chmod fails.
pub fn install_npm_tool(
    dev_tools_dir: &Path,
    bin_dir: &Path,
    bin_name: &str,
    npm_name: &str,
    packages: &[&str],
) -> rootcause::Result<InstallOutcome> {
    let target_dir = crate::downloaders::npm::run(dev_tools_dir, npm_name, packages)?;
    let target = target_dir.join(bin_name);
    ytil_sys::file::ln_sf(&target, &bin_dir.join(bin_name))?;
    ytil_sys::file::chmod_x(target)?;
    Ok(InstallOutcome::NoArtifact)
}

/// Common install pattern for macOS `.app` bundles built from source.
///
/// 1. symlink the binary into `bin_dir`
/// 2. make it executable
/// 3. copy the `.app` bundle into `/Applications` with an atomic swap
///
/// # Errors
/// - If symlink creation fails.
/// - If chmod fails.
/// - If copy to `/Applications` fails.
pub fn install_macos_app(app: &Path, bin_dir: &Path, bin_name: &str) -> rootcause::Result<()> {
    let binary = app.join("Contents").join("MacOS").join(bin_name);

    ytil_sys::file::ln_sf(&binary, &bin_dir.join(bin_name))?;
    ytil_sys::file::chmod_x(&binary)?;

    let Some(app_filename) = app.file_name().and_then(|n| n.to_str()) else {
        bail!("app path has no valid UTF-8 file name: {}", app.display());
    };

    let applications_app = PathBuf::from(format!("/Applications/{app_filename}"));
    let applications_app_old = PathBuf::from(format!("/Applications/{app_filename}.old"));

    if applications_app_old.exists() {
        std::fs::remove_dir_all(&applications_app_old)
            .context("error removing old .app backup")
            .attach_with(|| format!("path={}", applications_app_old.display()))?;
    }

    if applications_app.is_symlink() {
        std::fs::remove_file(&applications_app)
            .context("error removing .app symlink")
            .attach_with(|| format!("path={}", applications_app.display()))?;
    } else if applications_app.exists() {
        std::fs::rename(&applications_app, &applications_app_old)
            .context("error renaming existing .app to .old")
            .attach_with(|| format!("from={}", applications_app.display()))
            .attach_with(|| format!("to={}", applications_app_old.display()))?;
    }

    ytil_cmd::silent_cmd("cp")
        .args(["-R", &app.display().to_string(), "/Applications/"])
        .status()
        .context("failed to spawn cp for .app bundle")?
        .exit_ok()
        .context("cp .app bundle to /Applications failed")
        .attach_with(|| format!("app={}", app.display()))?;

    if applications_app_old.exists() {
        std::fs::remove_dir_all(&applications_app_old)
            .context("error cleaning up old .app backup")
            .attach_with(|| format!("path={}", applications_app_old.display()))?;
    }

    Ok(())
}

/// Format phase timing summary line.
fn format_timing(start: Instant, past_install: Instant, health_check: Option<Duration>) -> String {
    format!(
        "install_time={:?} health_check_time={:?} total_time={:?}",
        past_install.duration_since(start),
        health_check,
        start.elapsed()
    )
}

#[cfg(test)]
mod tests {
    use std::io::BufRead;
    use std::io::BufReader;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    use super::*;
    use crate::downloaders::http;
    use crate::downloaders::http::deflate::ChecksumSource;
    use crate::downloaders::http::deflate::HttpDeflateOption;

    #[rstest::rstest]
    #[case::with_health_check(AnsiColors::Yellow)]
    #[case::without_health_check(AnsiColors::Magenta)]
    fn test_success_bin_name_when_no_artifact_is_downloaded_uses_green(#[case] warning_color: AnsiColors) {
        let outcome = InstallOutcome::NoArtifact;

        let name = outcome.success_bin_name("tool", warning_color);

        assert_eq!(name, "tool".green().bold().to_string());
    }

    #[rstest::rstest]
    #[case::verified(
        Some("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"),
        AnsiColors::Yellow,
        AnsiColors::Green
    )]
    #[case::verified_without_health_check(
        Some("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"),
        AnsiColors::Magenta,
        AnsiColors::Green
    )]
    #[case::unverified(None, AnsiColors::Yellow, AnsiColors::Yellow)]
    #[case::unverified_without_health_check(None, AnsiColors::Magenta, AnsiColors::Magenta)]
    fn test_success_bin_name_when_artifact_is_downloaded_uses_actual_verification(
        #[case] checksum: Option<&'static str>,
        #[case] warning_color: AnsiColors,
        #[case] expected_color: AnsiColors,
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

        let download = http::run(
            &url,
            &HttpDeflateOption::WriteTo { dest_path: &dest_path },
            source.as_ref(),
        )
        .unwrap();
        let name = InstallOutcome::Downloaded(download).success_bin_name("tool", warning_color);

        server.join().unwrap();
        assert_eq!(name, "tool".color(expected_color).bold().to_string());
        assert_eq!(std::fs::read(&dest_path).unwrap(), b"hello world");
    }

    #[test]
    fn test_download_when_checksum_mismatches_rejects_artifact_before_installing() {
        let (url, server) = serve_download(vec![
            "hello world",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ]);
        let checksums_url = format!("{url}/checksums");
        let source = ChecksumSource {
            checksums_url: &checksums_url,
            filename: "tool",
        };
        let dir = tempfile::tempdir().unwrap();
        let dest_path = dir.path().join("tool");

        let result = http::run(
            &url,
            &HttpDeflateOption::WriteTo { dest_path: &dest_path },
            Some(&source),
        );

        server.join().unwrap();
        let error = result.err().expect("checksum mismatch must fail");
        assert!(error.to_string().contains("error checksum mismatch"));
        assert!(!dest_path.exists());
    }

    fn serve_download(bodies: Vec<&'static str>) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for body in bodies {
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
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });

        (url, server)
    }
}
