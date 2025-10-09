//! Run workspace lints concurrently.
//!
//! Executes Clippy, `cargo-machete`, and `cargo-sort` in parallel threads
//! against the provided workspace path, then reports results in the static
//! order defined by the internal `LINTS` table (deterministic regardless of
//! finish order). Includes per-lint wall-clock timing (milliseconds).
//!
//! # Arguments
//! - First CLI argument: path to the Cargo workspace root to lint.
//!
//! # Returns
//! - Process exits 0 if all lints succeed; 1 if any lint fails or a thread panics.
//!
//! # Errors
//! - Initialization errors from [`color_eyre::install`] abort before spawning lints.
//!
//! # Rationale
//! Provides a single fast command (usable in git hooks / CI) aggregating core
//! maintenance lints without shell scripting.
use std::process::Command;
use std::process::Output;
use std::time::Duration;
use std::time::Instant;

use color_eyre::eyre::eyre;
use color_eyre::owo_colors::OwoColorize;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

type LintRunner = fn(&str) -> color_eyre::Result<Output, CmdError>;

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
const LINTS: &[(&str, LintRunner)] = &[
    ("clippy", |path| {
        Command::new("cargo")
            .args(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
            .current_dir(path)
            .exec()
    }),
    ("cargo-machete", |path| {
        // Using `cargo-machete` rather than `cargo machete` to avoid issues caused by passing the
        // `path`.
        Command::new("cargo-machete").args(["--with-metadata", path]).exec()
    }),
    ("cargo-sort", |path| {
        Command::new("cargo-sort")
            .args(["--workspace", "--check", "--check-format"])
            .current_dir(path)
            .exec()
    }),
];

/// Result of a single lint execution with timing.
struct LintRun {
    duration: Duration,
    result: color_eyre::Result<Output, CmdError>,
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    let path = args
        .first()
        .ok_or_else(|| eyre!("missing required path arg in args={args:#?}"))?;

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
        .map(|(lint, run)| {
            let path = path.clone();
            let name_copy = *lint;
            let lint_handle = std::thread::spawn(move || {
                let start = Instant::now();
                let res = run(&path);
                LintRun {
                    duration: start.elapsed(),
                    result: res,
                }
            });
            (name_copy, lint_handle)
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

/// Report a single lint result produced by a joined thread.
///
/// Prints a colored success or error line including status code, duration (ms), and stripped stdout.
/// Returns true if an error (lint failure or join panic) occurred.
///
/// # Arguments
/// - `lint_name`: Logical name of the lint (e.g. "clippy").
/// - `lint_res`: The join result wrapping a `LintRun` (timed result).
///
/// # Rationale
/// Isolates formatting concerns from the control flow in [`main`].
fn report(lint_name: &str, lint_res: &std::thread::Result<LintRun>) -> bool {
    match lint_res {
        Ok(LintRun {
            duration,
            result: Ok(output),
        }) => {
            println!(
                "{} {} {} {}",
                format!("Success {lint_name}").green().bold(),
                format!("time={}ms", duration.as_millis()).white().bold(),
                format!("status={:?}", output.status.code()).white().bold(),
                format!(
                    "stdout={:?}",
                    str::from_utf8(&strip_ansi_escapes::strip(&output.stdout))
                )
                .white()
                .bold()
            );
            false
        }
        Ok(LintRun {
            duration,
            result: Err(error),
        }) => {
            eprintln!(
                "{} {} {}",
                format!("Error {lint_name}").red().bold(),
                format!("time={}ms", duration.as_millis()).white().bold(),
                format!("error={error}").white().bold()
            );
            true
        }
        Err(join_err) => {
            eprintln!("{}", format!("JoinHandle error={join_err:?}").red().bold());
            true
        }
    }
}
