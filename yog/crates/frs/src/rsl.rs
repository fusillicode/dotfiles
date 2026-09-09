//! The `frs rsl` command and its command-line interface.

use std::ffi::OsString;
use std::path::PathBuf;

use rootcause::report;
use ytil_sys::pico_args::Arguments;

mod ast;
mod engine;
mod rules;

/// Runs `frs rsl`.
///
/// Returns the JSON-ready lint violations for the supplied files.
///
/// # Errors
///
/// Returns an error when the arguments are invalid or a source file cannot be read or parsed.
pub fn run(mut cli_args: Arguments) -> rootcause::Result<Vec<serde_json::Value>> {
    if cli_args.contains("--help") {
        print!(include_str!("../rsl-help.txt"));
        return Ok(Vec::new());
    }

    let opts = RslOpts::try_from(cli_args.finish())?;
    crate::rsl::engine::check_paths(&opts.paths)
}

#[derive(Debug)]
struct RslOpts {
    paths: Vec<PathBuf>,
}

impl TryFrom<Vec<OsString>> for RslOpts {
    type Error = rootcause::Report;

    fn try_from(raw: Vec<OsString>) -> Result<Self, Self::Error> {
        let mut before_separator = Vec::new();
        let mut after_separator = Vec::new();
        let mut separator_seen = false;

        for argument in raw {
            if separator_seen {
                after_separator.push(argument);
            } else if argument == "--" {
                separator_seen = true;
            } else {
                before_separator.push(argument);
            }
        }

        let cli_args = Arguments::from_vec(before_separator);
        let mut paths = cli_args.finish();
        if let Some(option) = paths.iter().find(|path| path.to_string_lossy().starts_with('-')) {
            return Err(report!("unknown rsl option").attach(format!("option={}", option.to_string_lossy())));
        }
        paths.extend(after_separator);

        if paths.is_empty() {
            return Err(report!("expected one or more Rust source files"));
        }

        Ok(Self {
            paths: paths.into_iter().map(PathBuf::from).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fmt::Display;
    use std::path::PathBuf;

    use serde_json::Value;
    use tempfile::TempDir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_rsl_when_multiple_files_are_clean_returns_no_output() {
        let directory = require(tempfile::tempdir());
        let first = require(write_source(
            &directory,
            "first.rs",
            r"
            use std::fmt;
            fn first() {}
            ",
        ));
        let second = require(write_source(
            &directory,
            "second.rs",
            r"
            mod external;
            mod inline {
                const VALUE: usize = 1;
            }
            ",
        ));

        assert_that!(
            run_rsl(vec![first.into_os_string(), second.into_os_string()]),
            ok(eq(""))
        );
    }

    #[test]
    fn test_rsl_when_file_has_violation_returns_json_violations() {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "sample.rs",
            r"
            fn run() {}
            const VALUE: usize = 1;
            ",
        ));

        let output = require(run_rsl(vec![source.clone().into_os_string()]));
        let json: Value = require(serde_json::from_str(&output));
        let expected_file = source.to_string_lossy().into_owned();

        assert_that!(
            json,
            eq(serde_json::json!([{
                "rule": "item_group",
                "file": expected_file,
                "line": 3,
                "column": 13,
                "message": "source item group is out of order",
                "details": {
                    "actual_group": "constants",
                    "expected_group": "items",
                    "item": "const"
                }
            }]))
        );
    }

    #[test]
    fn test_rsl_when_file_has_use_after_fn_returns_json_violations() {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "sample.rs",
            r"
            fn run() {}
            use std::fmt;
            ",
        ));

        let output = require(run_rsl(vec![source.clone().into_os_string()]));
        let json: Value = require(serde_json::from_str(&output));
        let expected_file = source.to_string_lossy().into_owned();

        assert_that!(
            json,
            eq(serde_json::json!([{
                "rule": "item_group",
                "file": expected_file,
                "line": 3,
                "column": 13,
                "message": "source item group is out of order",
                "details": {
                    "actual_group": "use",
                    "expected_group": "items",
                    "item": "use"
                }
            }]))
        );
    }

    #[test]
    fn test_rsl_when_source_is_malformed_returns_parse_error() {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "broken.rs",
            r"
            fn missing( {
            ",
        ));

        assert_that!(
            run_rsl(vec![source.into_os_string()]),
            err(displays_as(contains_substring("could not parse Rust source")))
        );
    }

    #[test]
    fn test_rsl_when_file_is_missing_returns_read_error() {
        let directory = require(tempfile::tempdir());
        let missing = directory.path().join("missing.rs");

        assert_that!(
            run_rsl(vec![missing.into_os_string()]),
            err(displays_as(contains_substring("could not read Rust source")))
        );
    }

    #[test]
    fn test_rsl_when_no_file_is_supplied_returns_usage_error() {
        assert_that!(
            run_rsl(Vec::new()),
            err(displays_as(contains_substring(
                "expected one or more Rust source files"
            )))
        );
    }

    fn write_source(directory: &TempDir, name: &str, source: &str) -> std::io::Result<PathBuf> {
        let path = directory.path().join(name);
        std::fs::write(&path, source).map(|()| path)
    }

    fn run_rsl(arguments: impl IntoIterator<Item = OsString>) -> rootcause::Result<String> {
        let violations = crate::rsl::run(Arguments::from_vec(arguments.into_iter().collect()))?;
        if violations.is_empty() {
            return Ok(String::new());
        }

        Ok(format!("{}\n", serde_json::to_string(&violations)?))
    }

    fn require<T, E: Display>(result: Result<T, E>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => {
                panic!("test setup failed: {error}");
            }
        }
    }
}
