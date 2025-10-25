//! Run workspace lint suite concurrently (check or fix modes).
//!
//! Executes lints against the Cargo workspace root auto–detected via [`ytil_system::get_workspace_root`].
//!
//! # Behavior
//! - Auto-detects workspace root (no positional CLI argument required).
//! - Supports two modes:
//!   - Check (default) runs non-mutating lints.
//!   - Fix (`--fix` CLI flag) runs the lints that support automatic fixes.
//! - Spawns one thread per lint; all run concurrently.
//! - Result reporting joins threads in declaration order; a long first lint can delay visible output, potentially
//!   giving a false impression of serial execution.
//! - Prints each lint result with: success/error, duration (`time=<Duration>`), status code, stripped stdout or error.
//! - Exits with code 1 if any lint command returns a non-zero status, any lint command invocation errors, or any lint
//!   thread panics; exits 0 otherwise.
//!
//! # Returns
//! - Process exit code communicates aggregate success (0) or failure (1).
//!
//! # Errors
//! - Initialization errors from [`color_eyre::install`].
//! - Workspace root discovery errors from [`ytil_system::get_workspace_root`].
//!
//! # Rationale
//! Provides a single fast command (usable in git hooks / CI) aggregating core maintenance lints (style, dependency
//! pruning, manifest ordering) without bespoke shell scripting.
//! Split check vs fix modes minimize hook latency while enabling quick remediation.
//! Adds deterministic, ordered reporting for stable output while retaining parallel execution for speed.

use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::time::Duration;
use std::time::Instant;

use color_eyre::owo_colors::OwoColorize;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

/// Function pointer type for a single lint command invocation.
///
/// Encapsulates a non-mutating check or (optionally) mutating fix routine executed against the workspace root.
///
/// # Arguments
/// - `&Path` Workspace root directory the lint operates within.
///
/// # Returns
/// - `Ok`([`LintFnSuccess`]) on success, providing either command output or a plain message.
/// - `Err`([`LintFnError`]) if process spawning or execution fails.
///
/// # Rationale
/// Using a simple function pointer keeps dynamic dispatch trivial and avoids boxing trait objects; closures remain
/// zero-cost and we can compose slices of `(name, LintFn)` without lifetime complications.
///
/// # Future Work
/// - Consider an enum encapsulating richer metadata (e.g. auto-fix capability flag) to filter sets without duplicating
///   entries across lists.
type LintFn = fn(&Path) -> Result<LintFnSuccess, LintFnError>;

/// Error type for lint function execution failures.
///
/// # Errors
/// - [`LintFnError::CmdError`] Process spawning or execution failure.
#[derive(Debug, thiserror::Error)]
enum LintFnError {
    #[error(transparent)]
    CmdError(#[from] CmdError),
    #[error("{0}")]
    PlainMsg(String),
}

/// Success result from lint function execution.
///
/// # Variants
/// - [`LintFnSuccess::CmdOutput`] Standard command output with status and streams.
/// - [`LintFnSuccess::PlainMsg`] Simple string message (currently unused).
enum LintFnSuccess {
    CmdOutput(Output),
    PlainMsg(String),
}

/// Shared `clippy` lint definition.
///
/// Performs a full workspace lint across all targets and features, denying any warnings (`-D warnings`). This is
/// intentionally strict so CI / hooks surface new warnings immediately rather than allowing gradual drift.
///
/// # Rationale
/// Centralizing the closure avoids duplication between [`LINTS_CHECK`] and [`LINTS_FIX`] and makes future flag
/// adjustments (adding `--tests`, changing deny set) a single-line change.
///
/// # Performance
/// `cargo clippy` can be relatively expensive versus formatting or sorting tools. Placing it first provides early
/// feedback for a potentially longest-running lint while other shorter lints execute concurrently.
const CLIPPY: (&str, LintFn) = ("clippy", |path| {
    Command::new("cargo")
        .args(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
        .current_dir(path)
        .exec()
        .map(LintFnSuccess::CmdOutput)
        .map_err(LintFnError::from)
});

/// Workspace lint check set.
///
/// Contains non-mutating lints safe for fast verification in hooks / CI:
///
/// Execution model:
/// - Each lint spawns in its own thread; parallelism maximizes throughput while retaining deterministic join &
///   reporting order defined by slice declaration order.
/// - All runners are started before any join to avoid head-of-line blocking caused by early long-running lints.
///
/// Output contract:
/// - Prints logical name, duration (`time=<Duration>`), status code, and stripped stdout or error.
/// - Aggregate process exit code is 1 if any lint fails (non-zero status or panic), else 0.
const LINTS_CHECK: &[(&str, LintFn)] = &[
    CLIPPY,
    ("cargo fmt", |path| {
        Command::new("cargo")
            .args(["fmt", "--check"])
            .current_dir(path)
            .exec()
            .map(LintFnSuccess::CmdOutput)
            .map_err(LintFnError::from)
    }),
    ("cargo-machete", |path| {
        // Using `cargo-machete` rather than `cargo machete` to avoid issues caused by passing the
        // `path`.
        Command::new("cargo-machete")
            .args(["--with-metadata", &path.display().to_string()])
            .exec()
            .map(LintFnSuccess::CmdOutput)
            .map_err(LintFnError::from)
    }),
    ("cargo-sort", |path| {
        Command::new("cargo-sort")
            .args(["--workspace", "--check", "--check-format"])
            .current_dir(path)
            .exec()
            .map(LintFnSuccess::CmdOutput)
            .map_err(LintFnError::from)
    }),
    ("cargo-sort-derives", |path| {
        Command::new("cargo-sort-derives")
            .args(["sort-derives", "--check"])
            .current_dir(path)
            .exec()
            .map(LintFnSuccess::CmdOutput)
            .map_err(LintFnError::from)
    }),
];

/// Workspace lint fix set.
///
/// Contains mutating lints for rapid remediation of formatting / unused dependencies / manifest ordering.
///
/// Execution model:
/// - Parallel thread spawn identical to [`LINTS_CHECK`]; ordering of slice elements defines deterministic join &
///   reporting sequence.
///
/// Output contract:
/// - Same reporting shape as [`LINTS_CHECK`]: name, duration snippet (`time=<Duration>`), status code, stripped stdout
///   or error.
/// - Aggregate process exit code is 1 if any lint fails (non-zero status or panic), else 0.
///
/// Rationale:
/// - Focused mutation set avoids accidentally introducing changes via check-only tools.
/// - Deterministic ordered output aids CI log diffing while retaining concurrency for speed.
/// - Mirrors structure of [`LINTS_CHECK`] for predictable maintenance (additions require updating both tables).
const LINTS_FIX: &[(&str, LintFn)] = &[
    CLIPPY,
    ("cargo fmt", |path| {
        Command::new("cargo")
            .args(["fmt"])
            .current_dir(path)
            .exec()
            .map(LintFnSuccess::CmdOutput)
            .map_err(LintFnError::from)
    }),
    ("cargo-machete", |path| {
        Command::new("cargo-machete")
            .args(["--fix", "--with-metadata", &path.display().to_string()])
            .exec()
            .map(LintFnSuccess::CmdOutput)
            .map_err(LintFnError::from)
    }),
    ("cargo-sort", |path| {
        Command::new("cargo-sort")
            .args(["--workspace"])
            .current_dir(path)
            .exec()
            .map(LintFnSuccess::CmdOutput)
            .map_err(LintFnError::from)
    }),
    ("cargo-sort-derives", |path| {
        Command::new("cargo-sort-derives")
            .args(["sort-derives"])
            .current_dir(path)
            .exec()
            .map(LintFnSuccess::CmdOutput)
            .map_err(LintFnError::from)
    }),
    ("rm-ds-store", |path| {
        let (removed_paths, errors) = ytil_system::rm_matching_files(path, ".DS_Store", &[".git", "target"], false);
        let mut success_out = String::new();
        for path in removed_paths {
            success_out.push_str(&format!("{} {path:?}\n", "Removed".green()));
        }
        let mut error_out = String::new();
        for (path, error) in &errors {
            error_out.push_str(&format!(
                "{} path {path:?} error={}\n",
                "Error removing".red(),
                format!("{error}").red()
            ));
        }
        if errors.is_empty() {
            Ok(LintFnSuccess::PlainMsg(success_out))
        } else {
            Err(LintFnError::PlainMsg(format!("{success_out}\n{error_out}")))
        }
    }),
];

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    let fix_mode = args.first().is_some_and(|s| s == "--fix");

    let (start_msg, lints) = if fix_mode {
        ("lints fix", LINTS_FIX)
    } else {
        ("lints check", LINTS_CHECK)
    };

    let workspace_root = ytil_system::get_workspace_root()?;

    println!(
        "\nRunning {} {} in {}\n",
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
                    move || run_and_report(lint_name, &workspace_root, *lint_fn)
                }),
            )
        })
        .collect();

    let mut errors_count: i32 = 0;
    for (_lint_name, handle) in lints_handles {
        match handle.join() {
            Ok(Ok(_)) => (),
            Ok(Err(_)) => {
                errors_count = errors_count.saturating_add(1);
            }
            Err(join_err) => {
                errors_count = errors_count.saturating_add(1);
                eprintln!(
                    "{} error={}",
                    "Error joining thread".red().bold(),
                    format!("{join_err:#?}").red()
                );
            }
        }
    }

    println!(); // Cosmetic spacing.

    if errors_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Run a single lint, measure its duration, and report immediately.
///
/// # Arguments
/// - `lint_name` Human-friendly logical name of the lint (e.g. "clippy").
/// - `path` Workspace root path the lint operates in.
/// - `run` Function pointer executing the lint and returning [`LintFnSuccess`] or [`LintFnError`].
///
/// # Returns
/// - `Ok`([`LintFnSuccess`]) on lint success.
/// - `Err`([`Box<LintFnError>`]) on lint failure.
///
/// # Rationale
/// Collapses the previous two‑step pattern (timing + later reporting) into one
/// function so thread closures stay minimal and result propagation is explicit.
/// This also prevents losing the error flag (a regression after refactor).
fn run_and_report(lint_name: &str, path: &Path, run: LintFn) -> Result<LintFnSuccess, Box<LintFnError>> {
    let start = Instant::now();
    let lint_res = run(path);
    report(lint_name, &lint_res, start.elapsed());
    lint_res.map_err(Box::new)
}

/// Format and print the result of a completed lint execution.
///
/// # Arguments
/// - `lint_name` Logical name of the lint (e.g. "clippy").
/// - `lint_res` Result returned by executing the [`LintFn`], a [`Result<LintFnSuccess, LintFnError>`].
/// - `elapsed` Wall‑clock duration of the lint.
///
/// # Rationale
/// Keeps output formatting separate from orchestration logic in [`main`]; enables
/// alternate reporters (JSON, terse) later without threading timing logic everywhere.
fn report(lint_name: &str, lint_res: &Result<LintFnSuccess, LintFnError>, elapsed: Duration) {
    match lint_res {
        Ok(LintFnSuccess::CmdOutput(output)) => {
            println!(
                "{} {} status={:?} \n{}",
                lint_name.green().bold(),
                format_timing(elapsed),
                output.status.code(),
                str::from_utf8(&output.stdout).unwrap_or_default()
            );
        }
        Ok(LintFnSuccess::PlainMsg(msg)) => {
            println!("{} {} \n{msg}", lint_name.green().bold(), format_timing(elapsed));
        }
        Err(error) => {
            eprintln!("{} {} \n{error}", lint_name.red().bold(), format_timing(elapsed));
        }
    }
}

/// Format lint duration into `time=<duration>` snippet (auto-scaled, no color).
///
/// # Arguments
/// - `duration` Wall-clock elapsed time for a single lint execution.
///
/// # Returns
/// - Plain string `time=<duration>` where `<duration>` uses [`Duration`]'s `Debug` formatting (e.g., `1.234s`,
///   `15.6ms`, `321µs`, `42ns`) providing concise human-readable units.
///
/// Note: Colorization (if any) is applied by the caller (e.g. in [`report`]) not here, keeping this helper suitable for
/// future machine-readable output modes.
///
/// # Rationale
/// - Improves readability vs raw integer milliseconds; preserves sub-ms precision.
/// - Uses stable standard library formatting (no custom scaling logic).
fn format_timing(duration: Duration) -> String {
    format!("time={duration:?}")
}
