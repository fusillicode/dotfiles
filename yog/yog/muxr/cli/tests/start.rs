use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use muxr_core::SessionName;
use muxr_core::SessionPaths;
use rootcause::prelude::ResultExt;
use rootcause::report;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(5);
const SOCKET_HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const SOCKET_HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

#[test]
fn test_muxr_start_when_session_is_reused_attaches_to_same_server_and_cleans_up_on_exit() -> rootcause::Result<()> {
    let home = tempfile::Builder::new()
        .prefix("muxr-cli.")
        .tempdir_in("/tmp")
        .context("failed to create muxr cli test home")?;
    let session: SessionName = "cli_public".parse()?;
    let paths = session_paths(home.path(), &session);

    let first = run_muxr(home.path(), ["start", session.as_ref()])?;
    assert2::assert!(first.status.success());
    let first_pid = read_pid(&paths)?;

    let second = run_muxr(home.path(), ["start", session.as_ref()])?;
    assert2::assert!(second.status.success());
    pretty_assertions::assert_eq!(read_pid(&paths)?, first_pid);

    let exit = run_muxr_with_stdin(home.path(), ["start", session.as_ref()], b"exit\n")?;
    assert2::assert!(exit.status.success());
    wait_for_cleanup(&paths)?;

    assert2::assert!(!paths.socket.exists());
    assert2::assert!(!paths.pid.exists());
    assert2::assert!(paths.layout.exists());
    Ok(())
}

#[test]
fn test_muxr_when_no_args_and_no_sessions_starts_default_session() -> rootcause::Result<()> {
    let home = tempfile::Builder::new()
        .prefix("muxr-cli.")
        .tempdir_in("/tmp")
        .context("failed to create muxr cli test home")?;
    let session = SessionName::default();
    let paths = session_paths(home.path(), &session);

    let output = run_muxr(home.path(), [])?;

    assert2::assert!(output.status.success());
    let _pid = read_pid(&paths)?;

    let exit = run_muxr_with_stdin(home.path(), ["start", session.as_ref()], b"exit\n")?;
    assert2::assert!(exit.status.success());
    wait_for_cleanup(&paths)?;
    Ok(())
}

fn run_muxr<const N: usize>(home: &Path, args: [&str; N]) -> rootcause::Result<Output> {
    let child = Command::new(env!("CARGO_BIN_EXE_muxr"))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn muxr cli test command")?;

    wait_for_muxr_output(child)
}

fn run_muxr_with_stdin<const N: usize>(home: &Path, args: [&str; N], input: &[u8]) -> rootcause::Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_muxr"))
        .args(args)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn muxr cli test command")?;

    let Some(mut stdin) = child.stdin.take() else {
        return Err(report!("failed to open muxr cli test stdin"));
    };
    stdin.write_all(input).context("failed to write muxr cli test stdin")?;
    drop(stdin);

    wait_for_muxr_output(child)
}

fn wait_for_muxr_output(mut child: Child) -> rootcause::Result<Output> {
    let started_at = Instant::now();

    loop {
        if child
            .try_wait()
            .context("failed to poll muxr cli test command")?
            .is_some()
        {
            return Ok(child
                .wait_with_output()
                .context("failed to collect muxr cli test command output")?);
        }

        if started_at.elapsed() > PROCESS_TIMEOUT {
            // Public CLI smoke tests must fail fast when launch or attach blocks.
            drop(child.kill());
            let output = child
                .wait_with_output()
                .context("failed to collect timed-out muxr cli test command output")?;
            return Err(report!("timed out waiting for muxr cli test command")
                .attach(format!("stdout={}", String::from_utf8_lossy(&output.stdout)))
                .attach(format!("stderr={}", String::from_utf8_lossy(&output.stderr))));
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn read_pid(paths: &SessionPaths) -> rootcause::Result<String> {
    wait_for_path(&paths.pid)?;
    Ok(fs::read_to_string(&paths.pid).context("failed to read muxr cli test pid")?)
}

fn session_paths(home: &Path, session: &SessionName) -> SessionPaths {
    let state_root = home.join(".local").join("state").join("muxr");
    let root = state_root.join("sessions").join(session.as_ref());

    SessionPaths {
        socket: state_root.join("s").join(socket_file_name(session)),
        pid: root.join("server.pid"),
        layout: root.join("layout.json"),
        panes: root.join("panes"),
        root,
    }
}

fn socket_file_name(session: &SessionName) -> String {
    format!("{:016x}.sock", socket_hash(session))
}

fn socket_hash(session: &SessionName) -> u64 {
    let mut hash = SOCKET_HASH_OFFSET;
    for byte in session.as_ref().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(SOCKET_HASH_PRIME);
    }
    hash
}

fn wait_for_cleanup(paths: &SessionPaths) -> rootcause::Result<()> {
    let started_at = Instant::now();

    loop {
        if !paths.socket.exists() && !paths.pid.exists() {
            return Ok(());
        }

        if started_at.elapsed() > PROCESS_TIMEOUT {
            return Err(report!("timed out waiting for muxr cli test cleanup"));
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_path(path: &Path) -> rootcause::Result<()> {
    let started_at = Instant::now();

    loop {
        if path.exists() {
            return Ok(());
        }

        if started_at.elapsed() > PROCESS_TIMEOUT {
            return Err(report!("timed out waiting for muxr cli test path").attach(path.display().to_string()));
        }

        thread::sleep(Duration::from_millis(10));
    }
}
