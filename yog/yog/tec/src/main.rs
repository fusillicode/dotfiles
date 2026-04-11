//! Run workspace lint suite concurrently (check or fix modes).
//!
//! # Errors
//! - Workspace root discovery or lint execution fails.

use std::ops::Deref;
use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::time::Duration;
use std::time::Instant;

use owo_colors::OwoColorize;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;
use ytil_sys::cli::Args;

/// File suffixes considered Rust-related for conditional lint gating.
const RUST_EXTENSIONS: &[&str] = &[".rs", "Cargo.toml"];

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
const LINTS_CHECK: &[(&str, LintBuilder)] = &[
    ("clippy", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
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
    ("cargo fmt", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
            LintFnResult::from(
                Command::new("cargo")
                    .args(["fmt", "--check"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("cargo-machete", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
            LintFnResult::from(
                // Using `cargo-machete` rather than `cargo machete` to avoid issues caused by passing the
                // `path`.
                Command::new("cargo-machete")
                    .args(["--with-metadata", &path.display().to_string()])
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("cargo-sort", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
            LintFnResult::from(
                Command::new("cargo-sort")
                    .args(["--workspace", "--check", "--check-format"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("cargo-sort-derives", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
            LintFnResult::from(
                Command::new("cargo-sort-derives")
                    .args(["sort-derives", "--check"])
                    .current_dir(path)
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("rust-doc-build", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
            LintFnResult::from(
                nomicon::generate_rust_doc(path)
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
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
const LINTS_FIX: &[(&str, LintBuilder)] = &[
    ("clippy", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
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
    ("cargo fmt", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
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
    ("cargo-machete", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
            LintFnResult::from(
                Command::new("cargo-machete")
                    .args(["--fix", "--with-metadata", &path.display().to_string()])
                    .exec()
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
    ("cargo-sort", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
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
    ("cargo-sort-derives", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
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
    ("rust-doc-build", |changed_paths| {
        build_conditional_lint(changed_paths, RUST_EXTENSIONS, |path| {
            LintFnResult::from(
                nomicon::generate_rust_doc(path)
                    .map(LintFnSuccess::CmdOutput)
                    .map_err(LintFnError::from),
            )
        })
    }),
];

/// No-operation lint that reports "skipped" status.
const LINT_NO_OP: Lint = |_| LintFnResult(Ok(LintFnSuccess::PlainMsg(format!("{}\n", "skipped".bold()))));

/// Function pointer type for a single lint invocation.
type Lint = fn(&Path) -> LintFnResult;

/// Function pointer type for building lints based on file changes.
type LintBuilder = fn(&[String]) -> Lint;

/// Newtype wrapper around [`Result<LintFnSuccess, LintFnError>`].
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

/// Error type for [`Lint`] function execution failures.
///
/// # Errors
/// - [`LintFnError::CmdError`] Process spawning or execution failure.
#[derive(Debug, thiserror::Error)]
enum LintFnError {
    #[error(transparent)]
    CmdError(Box<CmdError>),
}

impl From<CmdError> for LintFnError {
    fn from(err: CmdError) -> Self {
        Self::CmdError(Box::new(err))
    }
}

impl From<Box<CmdError>> for LintFnError {
    fn from(err: Box<CmdError>) -> Self {
        Self::CmdError(err)
    }
}

/// Success result from [`Lint`] function execution.
///
/// # Variants
/// - [`LintFnSuccess::CmdOutput`] Standard command output with status and streams.
/// - [`LintFnSuccess::PlainMsg`] Simple string message.
enum LintFnSuccess {
    CmdOutput(Output),
    PlainMsg(String),
}

/// Conditionally returns the supplied lint or [`LINT_NO_OP`] based on file changes.
///
/// An empty `extensions` slice means the lint is unconditional (always runs).
fn build_conditional_lint(changed_paths: &[String], extensions: &[&str], lint: Lint) -> Lint {
    if extensions.is_empty()
        || changed_paths
            .iter()
            .any(|path| extensions.iter().any(|ext| path.ends_with(ext)))
    {
        lint
    } else {
        LINT_NO_OP
    }
}

/// Run a single lint, measure its duration, and report immediately.
fn run_and_report(lint_name: &str, path: &Path, run: Lint) -> LintFnResult {
    let start = Instant::now();
    let lint_res = run(path);
    report(lint_name, &lint_res, start.elapsed());
    lint_res
}

/// Format and print the result of a completed lint execution.
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
        Err(err) => {
            eprintln!("{} {} \n{err}", lint_name.red().bold(), format_timing(elapsed));
        }
    }
}

/// Format lint duration into `time=<duration>` snippet.
fn format_timing(duration: Duration) -> String {
    format!("time={duration:?}")
}

/// Run workspace lint suite concurrently (check or fix modes).
#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }
    let fix_mode = args.first().is_some_and(|s| s == "fix");

    let workspace_root = ytil_sys::dir::get_workspace_root()?;

    let repo = ytil_git::repo::discover(&workspace_root)?;
    let changed_paths = repo
        .statuses(None)?
        .iter()
        .filter_map(|entry| entry.path().map(str::to_string))
        .collect::<Vec<_>>();

    let (start_msg, lints) = if fix_mode {
        ("lints fix", LINTS_FIX)
    } else {
        ("lints check", LINTS_CHECK)
    };

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
        .map(|(lint_name, lint_builder)| {
            (
                lint_name,
                std::thread::spawn({
                    let workspace_root = workspace_root.clone();
                    let changed_paths = changed_paths.clone();
                    move || run_and_report(lint_name, &workspace_root, lint_builder(&changed_paths))
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
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::multiple_files_no_extension_filter(
        &["README.md".to_string(), "src/main.rs".to_string()],
        &[] as &[&str],
        "dummy success"
    )]
    #[case::multiple_files_with_rs_extension_filter(
        &["README.md".to_string(), "src/main.rs".to_string()],
        &[".rs"],
        "dummy success"
    )]
    #[case::single_non_rs_file_with_rs_extension_filter(
        &["README.md".to_string()],
        &[".rs"],
        "skipped"
    )]
    #[case::cargo_toml_change_triggers_rust_extensions(
        &["yog/yog/tec/Cargo.toml".to_string()],
        RUST_EXTENSIONS,
        "dummy success"
    )]
    #[case::non_rust_file_with_rust_extensions(
        &["README.md".to_string()],
        RUST_EXTENSIONS,
        "skipped"
    )]
    fn build_conditional_lint_returns_expected_result(
        #[case] changed_paths: &[String],
        #[case] extensions: &[&str],
        #[case] expected: &str,
    ) {
        let result_lint = build_conditional_lint(changed_paths, extensions, dummy_lint);
        let lint_result = result_lint(Path::new("/tmp"));

        assert2::assert!(let Ok(LintFnSuccess::PlainMsg(msg)) = lint_result.0);
        // Using contains instead of exact match because [`NO_OP`] [`Lint`] returns a colorized [`String`].
        assert!(msg.contains(expected));
    }

    fn dummy_lint(_path: &Path) -> LintFnResult {
        LintFnResult(Ok(LintFnSuccess::PlainMsg("dummy success".to_string())))
    }
}
