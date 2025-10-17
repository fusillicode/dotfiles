//! Run workspace lint suite concurrently.
//!
//! Executes lints against the Cargo workspace root auto–detected via [`ytil_system::get_workspace_root`].
//!
//! # Behavior
//! - Auto-detects workspace root (no positional CLI argument required).
//! - Spawns one thread per lint; all run concurrently.
//! - Result reporting joins threads in declaration order; a long first lint can delay visible output, potentially
//!   giving a false impression of serial execution.
//! - Prints each lint result with: success/error, duration (`time=<Duration>`), status code, stripped stdout or error.
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
//! Adds deterministic, ordered reporting for stable output while retaining parallel execution for speed.
use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::time::Duration;
use std::time::Instant;

use color_eyre::owo_colors::OwoColorize;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

type LintFn = fn(&Path) -> color_eyre::Result<Output, CmdError>;

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
const LINTS_CHECK: &[(&str, LintFn)] = &[
    ("clippy", |path| {
        Command::new("cargo")
            .args(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
            .current_dir(path)
            .exec()
    }),
    ("cargo fmt", |path| {
        Command::new("cargo").args(["fmt", "--check"]).current_dir(path).exec()
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
    ("cargo-sort-derives", |path| {
        Command::new("cargo-sort-derives")
            .args(["sort-derives", "--check"])
            .current_dir(path)
            .exec()
    }),
];

const LINTS_FIX: &[(&str, LintFn)] = &[
    ("cargo fmt", |path| {
        Command::new("cargo").args(["fmt"]).current_dir(path).exec()
    }),
    ("cargo-machete", |path| {
        Command::new("cargo-machete")
            .args(["--fix", "--with-metadata", &path.display().to_string()])
            .exec()
    }),
    ("cargo-sort", |path| {
        Command::new("cargo-sort")
            .args(["--workspace"])
            .current_dir(path)
            .exec()
    }),
    ("cargo-sort-derives", |path| {
        Command::new("cargo-sort-derives")
            .args(["sort-derives"])
            .current_dir(path)
            .exec()
    }),
];

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    let fix_mode = args.first().is_some_and(|s| s == "--fix");

    let (start_msg, lints) = if fix_mode {
        ("Lints fix", LINTS_FIX)
    } else {
        ("Lints check", LINTS_CHECK)
    };

    let workspace_root = ytil_system::get_workspace_root()?;

    println!(
        "\n{} {} in {}\n",
        start_msg.cyan().bold(),
        format!("{:#?}", lints.iter().map(|(lint, _)| lint).collect::<Vec<_>>())
            .white()
            .bold(),
        workspace_root.display().to_string().white().bold(),
    );

    // Spawn all lints in parallel.
    let lints_handles: Vec<_> = lints
        .iter()
        .map(|(lint_name, lint_fn)| {
            (
                lint_name,
                std::thread::spawn({
                    let workspace_root = workspace_root.clone();
                    move || run_and_time_lint(&workspace_root, *lint_fn)
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
/// [`TimedLintFn`] bundling the underlying lint result with elapsed duration.
///
/// # Rationale
/// Centralizes timing logic so the thread spawn closure stays minimal and
/// future changes (e.g. high-resolution timing, tracing spans) occur in one place.
fn run_and_time_lint(path: &Path, run: LintFn) -> TimedLintFn {
    let start = Instant::now();
    let res = run(path);
    TimedLintFn {
        duration: start.elapsed(),
        result: res,
    }
}

/// Result of a single lint execution with timing.
struct TimedLintFn {
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
/// - `lint_res` The join result wrapping a [`LintFn`] (timed result).
///
/// # Rationale
/// Isolates formatting concerns from the control flow in [`main`].
fn report(lint_name: &str, lint_res: &std::thread::Result<TimedLintFn>) -> bool {
    match lint_res {
        Ok(TimedLintFn {
            duration,
            result: Ok(output),
        }) => {
            println!(
                "{} {} status={:?} \n{}",
                lint_name.green().bold(),
                format_timing(*duration),
                output.status.code(),
                str::from_utf8(&output.stdout).unwrap_or_default()
            );
            false
        }
        Ok(TimedLintFn {
            duration,
            result: Err(error),
        }) => {
            eprintln!("{} {} \n{error}", lint_name.red().bold(), format_timing(*duration));
            true
        }
        Err(join_err) => {
            eprintln!("{}", format!("JoinHandle error={join_err:?}").red().bold());
            true
        }
    }
}

/// Format lint duration into colored `time=<duration>` snippet (auto-scaled).
///
/// # Arguments
/// - `duration` Wall-clock elapsed time for a single lint execution.
///
/// # Returns
/// - Colored string `time=<duration>` where `<duration>` uses [`Duration`]'s `Debug` formatting (e.g., `1.234s`,
///   `15.6ms`, `321µs`, `42ns`) providing concise human-readable units.
///
/// # Rationale
/// - Improves readability vs raw integer milliseconds; preserves sub-ms precision.
/// - Uses stable standard library formatting (no custom scaling logic).
/// - Keeps formatting centralized for future JSON / machine-output additions.
fn format_timing(duration: Duration) -> String {
    format!("time={duration:?}")
}
