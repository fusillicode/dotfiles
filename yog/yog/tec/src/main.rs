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

use std::fmt::Write;
use std::ops::Deref;
use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::time::Duration;
use std::time::Instant;

use color_eyre::owo_colors::OwoColorize;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;
use ytil_system::RmFilesOutcome;

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
const LINTS_CHECK: &[(&str, ConditionalLint)] = &[
    ("clippy", |_| {
        |path| {
            LintFnResult::from(
                Command::new("cargo")
                    .args(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        }
    }),
    ("cargo fmt", |_| {
        |path| {
            LintFnResult::from(
                Command::new("cargo")
                    .args(["fmt", "--check"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        }
    }),
    ("cargo-machete", |_| {
        |path| {
            LintFnResult::from(
                // Using `cargo-machete` rather than `cargo machete` to avoid issues caused by passing the
                // `path`.
                Command::new("cargo-machete")
                    .args(["--with-metadata", &path.display().to_string()])
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        }
    }),
    ("cargo-sort", |_| {
        |path| {
            LintFnResult::from(
                Command::new("cargo-sort")
                    .args(["--workspace", "--check", "--check-format"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        }
    }),
    ("cargo-sort-derives", |_| {
        |path| {
            LintFnResult::from(
                Command::new("cargo-sort-derives")
                    .args(["sort-derives", "--check"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        }
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
const LINTS_FIX: &[(&str, ConditionalLint)] = &[
    ("clippy", |changes| {
        conditional_lint(changes, Some(".rs"), |path| {
            LintFnResult::from(
                Command::new("cargo")
                    .args(["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("cargo fmt", |changes| {
        conditional_lint(changes, Some(".rs"), |path| {
            LintFnResult::from(
                Command::new("cargo")
                    .args(["fmt"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("cargo-machete", |changes| {
        conditional_lint(changes, Some(".rs"), |path| {
            LintFnResult::from(
                Command::new("cargo-machete")
                    .args(["--fix", "--with-metadata", &path.display().to_string()])
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("cargo-sort", |changes| {
        conditional_lint(changes, Some(".rs"), |path| {
            LintFnResult::from(
                Command::new("cargo-sort")
                    .args(["--workspace"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("cargo-sort-derives", |changes| {
        conditional_lint(changes, Some(".rs"), |path| {
            LintFnResult::from(
                Command::new("cargo-sort-derives")
                    .args(["sort-derives"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("rm-ds-store", |changes| {
        conditional_lint(changes, None, |path| {
            LintFnResult::from(ytil_system::rm_matching_files(
                path,
                ".DS_Store",
                &[".git", "target"],
                false,
            ))
        })
    }),
];

/// No-operation lint that reports "skipped" status.
///
/// Used by [`conditional_lint`] when a lint should be skipped due to no relevant file changes.
///
/// # Rationale
/// Provides a reusable constant for skipped lints, avoiding duplication of the skip logic and ensuring consistent
/// output.
const LINT_NO_OP: Lint = |_| LintFnResult(Ok(LintFnSuccess::PlainMsg(format!("{}\n", "skipped".bold()))));

/// Function pointer type for a single lint invocation.
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
type Lint = fn(&Path) -> LintFnResult;

/// Function pointer type for a conditional lint invocation.
///
/// Encapsulates a lint that may be skipped based on file changes, returning a [`Lint`] function to execute.
///
/// # Arguments
/// - `&[String]` List of changed file paths as strings.
///
/// # Returns
/// - [`Lint`] function that either runs the lint or reports skipped status.
///
/// # Rationale
/// Enables efficient conditional execution of lints, avoiding unnecessary work when no relevant files have changed
/// while maintaining consistent output format.
type ConditionalLint = fn(&[String]) -> Lint;

/// Newtype wrapper around [`Result<LintFnSuccess, LintFnError>`].
///
/// Provides ergonomic conversions from [`RmFilesOutcome`] and [`Deref`] access to the inner result.
///
/// # Rationale
/// Wraps the result to enable custom conversions without orphan rule violations.
struct LintFnResult(Result<LintFnSuccess, LintFnError>);

impl Deref for LintFnResult {
    type Target = Result<LintFnSuccess, LintFnError>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<Result<LintFnSuccess, LintFnError>> for LintFnResult {
    fn from(value: Result<LintFnSuccess, LintFnError>) -> Self {
        Self(value)
    }
}

/// Converts [`RmFilesOutcome`] into [`LintFnResult`] for uniform lint result handling.
///
/// Builds a formatted message listing removed files and errors, then wraps in success if no errors or failure
/// otherwise.
///
/// # Rationale
/// Enables treating file removal operations as lint results without duplicating conversion logic.
impl From<RmFilesOutcome> for LintFnResult {
    fn from(value: RmFilesOutcome) -> Self {
        let mut msg = String::new();
        for path in value.removed {
            let _ = writeln!(&mut msg, "{} {}", "Removed".green(), path.display());
        }
        for (path, error) in &value.errors {
            let _ = writeln!(
                &mut msg,
                "{} path{} error={}",
                "Error removing".red(),
                path.as_ref().map(|p| format!(" {:?}", p.display())).unwrap_or_default(),
                format!("{error}").red()
            );
        }
        if value.errors.is_empty() {
            Self(Ok(LintFnSuccess::PlainMsg(msg)))
        } else {
            Self(Err(LintFnError::PlainMsg(msg)))
        }
    }
}

/// Error type for [`Lint`] function execution failures.
///
/// # Errors
/// - [`LintFnError::CmdError`] Process spawning or execution failure.
/// - [`LintFnError::PlainMsg`] Generic error with a plain message string.
#[derive(Debug, thiserror::Error)]
enum LintFnError {
    #[error(transparent)]
    CmdError(#[from] CmdError),
    #[error("{0}")]
    PlainMsg(String),
}

/// Success result from [`Lint`] function execution.
///
/// # Variants
/// - [`LintFnSuccess::CmdOutput`] Standard command output with status and streams.
/// - [`LintFnSuccess::PlainMsg`] Simple string message (currently unused).
enum LintFnSuccess {
    CmdOutput(Output),
    PlainMsg(String),
}

/// Conditionally returns the supplied lint or [`LINT_NO_OP`] based on file changes.
///
/// Returns the provided [`Lint`] function if no extension filter is set or if any changed file matches the specified
/// extension. Otherwise, returns [`LINT_NO_OP`].
///
/// # Arguments
/// - `changes` List of changed file paths as strings.
/// - `extension` Optional file extension; if present, lint runs only if any changed file ends with it.
/// - `lint` The [`Lint`] function to conditionally execute.
///
/// # Returns
/// - [`Lint`] function that either executes the provided lint or reports skipped status.
///
/// # Rationale
/// Enables efficient skipping of lints when no relevant files have changed, reducing unnecessary work while
/// maintaining deterministic output.
fn conditional_lint(changes: &[String], extension: Option<&str>, lint: Lint) -> Lint {
    match extension {
        Some(ext) if changes.iter().any(|x| x.ends_with(ext)) => lint,
        None => lint,
        _ => LINT_NO_OP,
    }
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
fn run_and_report(lint_name: &str, path: &Path, run: Lint) -> LintFnResult {
    let start = Instant::now();
    let lint_res = run(path);
    report(lint_name, &lint_res, start.elapsed());
    lint_res
}

/// Format and print the result of a completed lint execution.
///
/// # Arguments
/// - `lint_name` Logical name of the lint (e.g. "clippy").
/// - `lint_res` Result returned by executing the [`Lint`], a [`Result<LintFnSuccess, LintFnError>`].
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

/// Run workspace lint suite concurrently (check or fix modes).
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

    let repo = ytil_git::get_repo(&workspace_root)?;
    let changes = repo
        .statuses(None)?
        .iter()
        .filter_map(|entry| entry.path().map(str::to_string))
        .collect::<Vec<_>>();

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
        .map(|(lint_name, conditional_lint_fn)| {
            (
                lint_name,
                std::thread::spawn({
                    let workspace_root = workspace_root.clone();
                    let changes = changes.clone();
                    move || run_and_report(lint_name, &workspace_root, conditional_lint_fn(&changes))
                }),
            )
        })
        .collect();

    let mut errors_count: i32 = 0;
    for (_lint_name, handle) in lints_handles {
        match handle.join().as_deref() {
            Ok(Ok(_)) => (),
            Ok(Err(_)) => errors_count = errors_count.saturating_add(1),
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

#[cfg(test)]
mod tests {
    use std::io::Error;
    use std::io::ErrorKind;
    use std::path::PathBuf;

    use rstest::rstest;

    use super::*;

    #[test]
    fn from_rm_files_outcome_when_no_removed_no_errors_returns_success() {
        let outcome = RmFilesOutcome {
            removed: vec![],
            errors: vec![],
        };

        let result = LintFnResult::from(outcome);

        assert2::let_assert!(Ok(LintFnSuccess::PlainMsg(msg)) = result.0);
        pretty_assertions::assert_eq!(msg, "");
    }

    #[test]
    fn from_rm_files_outcome_when_some_removed_no_errors_returns_success() {
        let outcome = RmFilesOutcome {
            removed: vec![PathBuf::from("file1.txt"), PathBuf::from("file2.txt")],
            errors: vec![],
        };

        let result = LintFnResult::from(outcome);

        assert2::let_assert!(Ok(LintFnSuccess::PlainMsg(msg)) = result.0);
        assert!(msg.contains("Removed"));
        assert!(msg.contains("file1.txt"));
        assert!(msg.contains("file2.txt"));
    }

    #[test]
    fn from_rm_files_outcome_when_no_removed_some_errors_returns_failure() {
        let outcome = RmFilesOutcome {
            removed: vec![],
            errors: vec![(
                Some(PathBuf::from("badfile.txt")),
                Error::new(ErrorKind::PermissionDenied, "permission denied"),
            )],
        };

        let result = LintFnResult::from(outcome);

        assert2::let_assert!(Err(LintFnError::PlainMsg(msg)) = result.0);
        assert!(msg.contains("Error removing"));
        assert!(msg.contains("\"badfile.txt\""));
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn from_rm_files_outcome_when_error_without_path_returns_failure() {
        let outcome = RmFilesOutcome {
            removed: vec![],
            errors: vec![(None, Error::new(ErrorKind::NotFound, "file not found"))],
        };

        let result = LintFnResult::from(outcome);

        assert2::let_assert!(Err(LintFnError::PlainMsg(msg)) = result.0);
        assert!(msg.contains("Error removing"));
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn from_rm_files_outcome_when_mixed_removed_and_errors_returns_failure() {
        let outcome = RmFilesOutcome {
            removed: vec![PathBuf::from("goodfile.txt")],
            errors: vec![(Some(PathBuf::from("badfile.txt")), Error::other("some error"))],
        };

        let result = LintFnResult::from(outcome);

        assert2::let_assert!(Err(LintFnError::PlainMsg(msg)) = result.0);
        assert!(msg.contains("Removed"));
        assert!(msg.contains("goodfile.txt"));
        assert!(msg.contains("Error removing"));
        assert!(msg.contains("\"badfile.txt\""));
        assert!(msg.contains("some error"));
    }

    #[rstest]
    #[case(vec!["README.md".to_string(), "src/main.rs".to_string()], None, "dummy success")]
    #[case(vec!["README.md".to_string(), "src/main.rs".to_string()], Some(".rs"), "dummy success")]
    #[case(vec!["README.md".to_string()], Some(".rs"), "skipped")]
    fn conditional_lint_returns_expected_result(
        #[case] changes: Vec<String>,
        #[case] extension: Option<&str>,
        #[case] expected: &str,
    ) {
        let result_lint = conditional_lint(&changes, extension, dummy_lint);
        let lint_result = result_lint(Path::new("/tmp"));

        assert2::let_assert!(Ok(LintFnSuccess::PlainMsg(msg)) = lint_result.0);
        // Using contains instead of exact match because [`NO_OP`] [`Lint`] returns a colorized [`String`].
        assert!(msg.contains(expected));
    }

    fn dummy_lint(_path: &Path) -> LintFnResult {
        LintFnResult(Ok(LintFnSuccess::PlainMsg("dummy success".to_string())))
    }
}
