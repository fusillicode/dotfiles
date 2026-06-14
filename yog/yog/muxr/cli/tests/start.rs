use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::sync::Mutex;
use std::sync::OnceLock;
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
const SERVER_EXECUTABLE: &str = "muxr-server";

static SERVER_BUILD: OnceLock<()> = OnceLock::new();
static SERVER_BUILD_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_muxr_start_when_session_is_reused_attaches_to_same_server_and_cleans_up_on_exit() -> rootcause::Result<()> {
    let home = tempfile::Builder::new()
        .prefix("muxr-cli.")
        .tempdir_in("/tmp")
        .context("failed to create muxr cli test home")?;
    let session: SessionName = "cli_public".parse()?;
    let paths = session_paths(home.path(), &session);

    let first = run_muxr(home.path(), ["start", session.as_ref()])?;
    assert_success("first start", &first)?;
    let first_pid = read_pid(&paths)?;

    let second = run_muxr(home.path(), ["start", session.as_ref()])?;
    assert_success("second start", &second)?;
    pretty_assertions::assert_eq!(read_pid(&paths)?, first_pid);

    let exit = run_muxr_with_stdin(home.path(), ["start", session.as_ref()], b"exit\n")?;
    assert_success("exit start", &exit)?;
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

    assert_success("default start", &output)?;
    let _pid = read_pid(&paths)?;

    let exit = run_muxr_with_stdin(home.path(), ["start", session.as_ref()], b"exit\n")?;
    assert_success("default exit", &exit)?;
    wait_for_cleanup(&paths)?;
    Ok(())
}

fn run_muxr<const N: usize>(home: &Path, args: [&str; N]) -> rootcause::Result<Output> {
    self::ensure_server_binary()?;
    let child = Command::new(env!("CARGO_BIN_EXE_muxr"))
        .args(args)
        .env("HOME", home)
        .env("COLUMNS", "80")
        .env("LINES", "24")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn muxr cli test cmd")?;

    wait_for_muxr_output(child)
}

fn run_muxr_with_stdin<const N: usize>(home: &Path, args: [&str; N], input: &[u8]) -> rootcause::Result<Output> {
    self::ensure_server_binary()?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_muxr"))
        .args(args)
        .env("HOME", home)
        .env("COLUMNS", "80")
        .env("LINES", "24")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn muxr cli test cmd")?;

    let Some(mut stdin) = child.stdin.take() else {
        return Err(report!("failed to open muxr cli test stdin"));
    };
    stdin.write_all(input).context("failed to write muxr cli test stdin")?;
    drop(stdin);

    wait_for_muxr_output(child)
}

fn ensure_server_binary() -> rootcause::Result<()> {
    if SERVER_BUILD.get().is_some() {
        return Ok(());
    }

    let _guard = SERVER_BUILD_LOCK
        .lock()
        .map_err(|_| report!("muxr server build lock poisoned"))?;
    if SERVER_BUILD.get().is_none() {
        self::build_server_binary()?;
        SERVER_BUILD
            .set(())
            .map_err(|()| report!("muxr server build cache was already initialized"))?;
    }
    Ok(())
}

fn build_server_binary() -> rootcause::Result<()> {
    // The public muxr binary starts a sibling server runner; existence alone is not enough because a stale binary may
    // be left from a previous bin name or source revision. Build once in the same profile as the tested CLI binary.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = workspace_manifest_string()?;
    let muxr_executable = Path::new(env!("CARGO_BIN_EXE_muxr"));
    let profile_dir = muxr_executable.parent().ok_or_else(|| {
        report!("muxr test binary has no parent dir").attach(format!("path={}", muxr_executable.display()))
    })?;
    let target_dir = profile_dir.parent().ok_or_else(|| {
        report!("muxr test binary has no target dir").attach(format!("path={}", muxr_executable.display()))
    })?;

    let mut cmd = Command::new(cargo);
    cmd.args([
        "build",
        "--manifest-path",
        manifest.as_str(),
        "-p",
        "muxr-server",
        "--bin",
        SERVER_EXECUTABLE,
    ]);
    // Coverage/nextest can run `muxr` from a target dir that differs from inherited `CARGO_TARGET_DIR`; build the
    // sibling server into the tested binary's target dir so the production sibling lookup is exercised.
    cmd.arg("--target-dir").arg(target_dir);
    if profile_dir.file_name().is_some_and(|profile| profile == "release") {
        cmd.arg("--release");
    }

    let status = cmd.status().context("failed to build muxr server for cli smoke")?;
    if !status.success() {
        return Err(report!("muxr server build failed").attach(format!("status={status}")));
    }
    Ok(())
}

fn workspace_manifest_string() -> rootcause::Result<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("Cargo.toml")
        .canonicalize()
        .context("failed to resolve yog workspace manifest")?;
    Ok(manifest.to_string_lossy().into_owned())
}

fn wait_for_muxr_output(mut child: Child) -> rootcause::Result<Output> {
    let started_at = Instant::now();

    loop {
        if child.try_wait().context("failed to poll muxr cli test cmd")?.is_some() {
            return Ok(child
                .wait_with_output()
                .context("failed to collect muxr cli test cmd output")?);
        }

        if started_at.elapsed() > PROCESS_TIMEOUT {
            // Public CLI smoke tests must fail fast when launch or attach blocks.
            drop(child.kill());
            let output = child
                .wait_with_output()
                .context("failed to collect timed-out muxr cli test cmd output")?;
            return Err(report!("timed out waiting for muxr cli test cmd")
                .attach(format!("stdout={}", String::from_utf8_lossy(&output.stdout)))
                .attach(format!("stderr={}", String::from_utf8_lossy(&output.stderr))));
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_success(context: &str, output: &Output) -> rootcause::Result<()> {
    if output.status.success() {
        return Ok(());
    }

    Err(report!("muxr cli test command failed")
        .attach(format!("context={context}"))
        .attach(format!("status={}", output.status))
        .attach(format!("stdout={}", String::from_utf8_lossy(&output.stdout)))
        .attach(format!("stderr={}", String::from_utf8_lossy(&output.stderr))))
}

fn read_pid(paths: &SessionPaths) -> rootcause::Result<String> {
    wait_for_path(&paths.pid)?;
    Ok(fs::read_to_string(&paths.pid).context("failed to read muxr cli test pid")?)
}

fn session_paths(home: &Path, session: &SessionName) -> SessionPaths {
    let state_root = home.join(".local").join("state").join("muxr");
    let root = state_root.join("sessions").join(session.as_ref());
    let mut socket_hash = SOCKET_HASH_OFFSET;
    for byte in session.as_ref().bytes() {
        socket_hash ^= u64::from(byte);
        socket_hash = socket_hash.wrapping_mul(SOCKET_HASH_PRIME);
    }

    SessionPaths {
        socket: state_root.join("s").join(format!("{socket_hash:016x}.sock")),
        pid: root.join("server.pid"),
        layout: root.join("layout.json"),
        panes: root.join("panes"),
        root,
    }
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
