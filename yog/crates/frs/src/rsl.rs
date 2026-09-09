//! The `frs rsl` command and its command-line interface.

use std::ffi::OsString;
use std::path::PathBuf;

use rootcause::report;
use ytil_sys::pico_args::Arguments;

use crate::rsl::rules::RuleViolation;

mod ast;
mod engine;
mod rules;

/// Runs `frs rsl`.
///
/// Returns the lint violations for the supplied files.
///
/// # Errors
///
/// Returns an error when the arguments are invalid or a source file cannot be read or parsed.
pub fn run(mut cli_args: Arguments) -> rootcause::Result<RslOutput> {
    if cli_args.contains("--help") {
        print!(include_str!("../rsl-help.txt"));
        return Ok(RslOutput::Compact { violations: Vec::new() });
    }

    let opts = RslOpts::try_from(cli_args.finish())?;
    let violations = crate::rsl::engine::check_paths(&opts.paths)?;

    Ok(match opts.format {
        OutputFormat::Compact => RslOutput::Compact { violations },
        OutputFormat::Json => RslOutput::Json { violations },
    })
}

#[derive(Debug)]
struct RslOpts {
    paths: Vec<PathBuf>,
    format: OutputFormat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Compact,
    Json,
}

pub enum RslOutput {
    Compact { violations: Vec<Box<dyn RuleViolation>> },
    Json { violations: Vec<Box<dyn RuleViolation>> },
}

impl RslOutput {
    pub const fn is_empty(&self) -> bool {
        match self {
            Self::Compact { violations } | Self::Json { violations } => violations.is_empty(),
        }
    }

    pub fn render(&self) -> serde_json::Result<String> {
        match self {
            Self::Compact { violations } => Ok(violations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")),
            Self::Json { violations } => {
                let serialized = violations
                    .iter()
                    .map(|violation| violation.to_json())
                    .collect::<serde_json::Result<Vec<_>>>()?;
                serde_json::to_string(&serialized)
            }
        }
    }
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

        let mut cli_args = Arguments::from_vec(before_separator);
        let format = if cli_args.contains("--json") {
            OutputFormat::Json
        } else {
            OutputFormat::Compact
        };
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
            format,
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
    fn test_rsl_when_file_has_violation_returns_compact_violations() {
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
        let expected_file = source.to_string_lossy();

        assert_that!(
            output,
            eq(format!(
                "{expected_file}:3:13 item group out of order - constants -> items [const]\n"
            ))
        );
    }

    #[test]
    fn test_rsl_when_json_flag_is_supplied_returns_json_violations() {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "sample.rs",
            r"
            fn run() {}
            use std::fmt;
            ",
        ));

        let output = require(run_rsl(vec![OsString::from("--json"), source.clone().into_os_string()]));
        let json: Value = require(serde_json::from_str(&output));
        let expected_file = source.to_string_lossy().into_owned();

        assert_that!(
            json,
            eq(serde_json::json!([{
                "rule": "item_group",
                "file": expected_file,
                "line": 3,
                "column": 13,
                "message": "item group out of order",
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
        let output = crate::rsl::run(Arguments::from_vec(arguments.into_iter().collect()))?;
        if output.is_empty() {
            return Ok(String::new());
        }

        Ok(format!("{}\n", output.render()?))
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
