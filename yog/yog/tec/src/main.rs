//! Run workspace lint suite concurrently.
//!
//! Executes lints against the Cargo workspace root auto–detected via [`ytil_system::get_workspace_root`].
//!
//! # Behavior
//! - Auto-detects workspace root (no positional CLI argument required).
//! - Spawns one thread per lint; all run concurrently.
//! - Prints each lint result with: success/error, duration, status code, stripped stdout or error.
//! - Exits 1 if any lint fails (non-zero exit status) or a thread panics; otherwise exits 0.
//!
//! # Returns
//! - Process exit code communicates aggregate success (0) or failure (1).
//!
//! # Errors
//! - Initialization errors from [`color_eyre::install`].
//! - Workspace root discovery errors from [`ytil_system::get_workspace_root`].
//!
//! # Rationale
//! Provides a single fast command (usable in git hooks / CI) aggregating core
//! maintenance lints (style, dependency pruning, manifest ordering) without
//! bespoke shell scripting.
use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::time::Duration;
use std::time::Instant;

use color_eyre::owo_colors::OwoColorize;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

type LintRun = fn(&Path) -> color_eyre::Result<Output, CmdError>;

/// Available workspace lints.
///
/// Each entry maps a human-readable lint name to a function that builds and
/// executes the corresponding command, returning its captured [`Output`].
///
/// - Order in this slice defines the (deterministic) order of result reporting.
/// - Execution itself is parallel: all runners are spawned before any joins.
///
/// # Rationale
/// Central list makes it trivial to add / remove lints and print them up-front
/// for user visibility.
const LINTS: &[(&str, LintRun)] = &[
    ("clippy", |path| {
        Command::new("cargo")
            .args(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
            .current_dir(path)
            .exec()
    }),
    ("cargo-machete", |path| {
        // Using `cargo-machete` rather than `cargo machete` to avoid issues caused by passing the
        // `path`.
        Command::new("cargo-machete")
            .args(["--with-metadata", &path.display().to_string()])
            .exec()
    }),
    ("cargo-sort", |path| {
        Command::new("cargo-sort")
            .args(["--workspace", "--check", "--check-format"])
            .current_dir(path)
            .exec()
    }),
];

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let workspace_root = ytil_system::get_workspace_root()?;

    println!(
        "{} {}\n",
        "Running lints:".cyan().bold(),
        format!("{:#?}", LINTS.iter().map(|(lint, _)| lint).collect::<Vec<_>>())
            .white()
            .bold()
    );

    // Spawn all lints in parallel.
    let lints_handles: Vec<_> = LINTS
        .iter()
        .map(|(lint_name, lint_run)| {
            (
                lint_name,
                std::thread::spawn({
                    let workspace_root = workspace_root.clone();
                    move || run_and_time_lint(&workspace_root, *lint_run)
                }),
            )
        })
        .collect();

    let mut errors = false;
    for (lint_name, handle) in lints_handles {
        let lint_res = handle.join();
        if lint_res.is_err() {
            errors = true;
        }
        if report(lint_name, &lint_res) {
            errors = true;
        }
    }

    println!(); // Cosmetic spacing.

    if errors {
        std::process::exit(1);
    }

    Ok(())
}

/// Execute a single lint runner and measure wall-clock duration.
///
/// # Arguments
/// - `path` Workspace root path the lint should operate in.
/// - `run` Function pointer executing the lint and returning its captured [`Output`].
///
/// # Returns
/// [`TimedLintRun`] bundling the underlying lint result with elapsed duration.
///
/// # Rationale
/// Centralizes timing logic so the thread spawn closure stays minimal and
/// future changes (e.g. high-resolution timing, tracing spans) occur in one place.
fn run_and_time_lint(path: &Path, run: LintRun) -> TimedLintRun {
    let start = Instant::now();
    let res = run(path);
    TimedLintRun {
        duration: start.elapsed(),
        result: res,
    }
}

/// Result of a single lint execution with timing.
struct TimedLintRun {
    duration: Duration,
    result: color_eyre::Result<Output, CmdError>,
}

/// Report a single lint result produced by a joined thread.
///
/// Prints a colored success or error line including status code, duration (ms), and stripped stdout.
/// Returns true if an error (lint failure or join panic) occurred.
///
/// # Arguments
/// - `lint_name` Logical name of the lint (e.g. "clippy").
/// - `lint_res` The join result wrapping a [`LintRun`] (timed result).
///
/// # Rationale
/// Isolates formatting concerns from the control flow in [`main`].
fn report(lint_name: &str, lint_res: &std::thread::Result<TimedLintRun>) -> bool {
    match lint_res {
        Ok(TimedLintRun {
            duration,
            result: Ok(output),
        }) => {
            println!(
                "{} {} {} \n{}",
                lint_name.green().bold(),
                report_duration(*duration),
                format!("status={:?}", output.status.code()).white().bold(),
                str::from_utf8(&output.stdout).unwrap_or_default()
            );
            false
        }
        Ok(TimedLintRun {
            duration,
            result: Err(error),
        }) => {
            eprintln!("{} {} \n{error}", lint_name.red().bold(), report_duration(*duration));
            true
        }
        Err(join_err) => {
            eprintln!("{}", format!("JoinHandle error={join_err:?}").red().bold());
            true
        }
    }
}

/// Format elapsed wall-clock duration (ms) for lint output.
///
/// # Arguments
/// - `duration` Elapsed time for a single lint run.
///
/// # Returns
/// Colored string of the form `took=<ms>ms`.
///
/// # Rationale
/// Single styling point keeps result lines uniform and simplifies future
/// formatting changes (e.g. alignment, units).
fn report_duration(duration: Duration) -> String {
    format!("took={}ms", duration.as_millis()).white().bold().to_string()
}
