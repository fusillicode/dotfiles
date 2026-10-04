//! The `frs rsl` command and its command-line interface.

use std::ffi::OsString;
use std::path::PathBuf;

use rootcause::report;
use ytil_sys::pico_args::Arguments;

pub use self::output::RslOutput;
use self::output::ViolationOutputFormat;
use self::rules::SelectedRules;

mod ast;
mod engine;
mod output;
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
        print!("{}", crate::cmds::Help::Rsl.text());
        return Ok(RslOutput::new(Vec::new(), ViolationOutputFormat::Compact));
    }

    let opts = match RslOpts::try_from(cli_args.finish()) {
        Ok(opts) => opts,
        Err(error) => {
            eprintln!("{}", crate::cmds::Help::Rsl.text());
            return Err(error);
        }
    };
    let violations = crate::cmds::rsl::engine::check_paths(&opts.paths, &opts.rules)?;
    let format = if opts.debug {
        ViolationOutputFormat::Debug
    } else {
        ViolationOutputFormat::Compact
    };

    Ok(RslOutput::new(violations, format))
}

struct RslOpts {
    debug: bool,
    paths: Vec<PathBuf>,
    rules: SelectedRules,
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
        let debug = cli_args.contains("--debug");
        let mut rule_ids = Vec::new();
        while let Some(rule_list) = cli_args.opt_value_from_str::<_, String>("--rules")? {
            rule_ids.extend(rule_list.split(',').map(str::to_owned));
        }
        let rules = SelectedRules::try_from(rule_ids)?;
        let mut paths = cli_args.finish();
        if let Some(option) = paths.iter().find(|path| path.to_string_lossy().starts_with('-')) {
            return Err(report!("unknown rsl option").attach(format!("option={}", option.to_string_lossy())));
        }
        paths.extend(after_separator);

        if paths.is_empty() {
            return Err(report!("expected one or more Rust source files"));
        }

        Ok(Self {
            debug,
            paths: paths.into_iter().map(PathBuf::from).collect(),
            rules,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fmt::Display;
    use std::path::PathBuf;

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
    fn test_rsl_when_multiple_files_have_violations_preserves_input_file_order() {
        let directory = require(tempfile::tempdir());
        let first = require(write_source(
            &directory,
            "first.rs",
            r"
            fn first() {}
            const FIRST: usize = 1;
            ",
        ));
        let second = require(write_source(
            &directory,
            "second.rs",
            r"
            fn second() {}
            const SECOND: usize = 2;
            ",
        ));

        let output = require(run_rsl(vec![
            first.clone().into_os_string(),
            second.clone().into_os_string(),
        ]));

        assert_that!(
            output,
            eq(format!(
                "{}:3:13,move `const` after `items`\n{}:3:13,move `const` after `items`\n",
                first.display(),
                second.display(),
            ))
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
        let expected_file = source.to_string_lossy().into_owned();

        assert_that!(output, eq(format!("{expected_file}:3:13,move `const` after `items`\n")));
    }

    #[test]
    fn test_rsl_when_function_qualification_is_invalid_omits_rule_codes_by_default() {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "sample.rs",
            r#"
            use tempfile::tempdir;
            fn main() {
                tempdir();
                std::fs::read_to_string("foo");
            }
            "#,
        ));

        let expected_file = source.to_string_lossy().into_owned();
        let output = require(run_rsl(vec![source.into_os_string()]));

        assert_that!(
            output,
            eq(format!(
                "{expected_file}:4:17,replace `tempdir` with `tempfile::tempdir`\n{expected_file}:5:17,replace `std::fs::read_to_string` with `fs::read_to_string`; add `use std::fs;`\n"
            ))
        );
    }

    #[test]
    fn test_rsl_when_debug_flag_is_passed_includes_rule_codes() {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "sample.rs",
            r#"
            use tempfile::tempdir;
            fn main() {
                tempdir();
                std::fs::read_to_string("foo");
            }
            "#,
        ));

        let expected_file = source.to_string_lossy().into_owned();
        let output = require(run_rsl(vec![OsString::from("--debug"), source.into_os_string()]));

        assert_that!(
            output,
            eq(format!(
                "{expected_file}:4:17,unqualified_call,replace `tempdir` with `tempfile::tempdir`\n{expected_file}:5:17,overqualified_call,replace `std::fs::read_to_string` with `fs::read_to_string`; add `use std::fs;`\n"
            ))
        );
    }

    #[rstest::rstest]
    #[case(
        vec!["--rules", "unqualified-call"],
        "{file}:2:13,unqualified_call,replace `tempdir` with `tempfile::tempdir`\n"
    )]
    #[case(
        vec!["--rules", "overqualified-call,unqualified-call"],
        "{file}:2:13,unqualified_call,replace `tempdir` with `tempfile::tempdir`\n\
         {file}:2:24,overqualified_call,replace `std::fs::read_to_string` with `fs::read_to_string`; \
         add `use std::fs;`\n"
    )]
    #[case(
        vec!["--rules", "overqualified-call", "--rules", "unqualified-call"],
        "{file}:2:13,unqualified_call,replace `tempdir` with `tempfile::tempdir`\n\
         {file}:2:24,overqualified_call,replace `std::fs::read_to_string` with `fs::read_to_string`; \
         add `use std::fs;`\n"
    )]
    #[case(
        vec!["--rules", "unqualified-call,unqualified-call", "--rules", "unqualified-call"],
        "{file}:2:13,unqualified_call,replace `tempdir` with `tempfile::tempdir`\n"
    )]
    #[case(vec!["--rules", "relative-path"], "")]
    fn test_rsl_when_rules_are_selected_runs_only_selected_rules(#[case] options: Vec<&str>, #[case] expected: &str) {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "sample.rs",
            "use tempfile::tempdir;\nfn main() { tempdir(); std::fs::read_to_string(\"foo\"); }",
        ));
        let expected = expected.replace("{file}", &source.to_string_lossy());
        let mut arguments: Vec<_> = options.into_iter().map(OsString::from).collect();
        arguments.push(OsString::from("--debug"));
        arguments.push(source.into_os_string());

        assert_that!(run_rsl(arguments), ok(eq(expected)));
    }

    #[rstest::rstest]
    #[case(vec!["--rules", "unknown"], "unknown rsl rule")]
    #[case(vec!["--rules", ""], "unknown rsl rule")]
    #[case(vec!["--rules", "unqualified_call"], "unknown rsl rule")]
    #[case(vec!["--rules", "unqualified-call,unknown"], "unknown rsl rule")]
    #[case(vec!["--rules", ",unqualified-call"], "unknown rsl rule")]
    #[case(vec!["--rules", "unqualified-call,"], "unknown rsl rule")]
    #[case(vec!["--rules", "unqualified-call,,relative-path"], "unknown rsl rule")]
    #[case(vec!["--rules"], "--rules")]
    fn test_rsl_when_rules_option_is_invalid_returns_usage_error(#[case] arguments: Vec<&str>, #[case] expected: &str) {
        assert_that!(
            run_rsl(arguments.into_iter().map(OsString::from)),
            err(displays_as(contains_substring(expected)))
        );
    }

    #[test]
    fn test_rsl_when_rules_option_is_after_separator_treats_it_as_a_path() {
        assert_that!(
            run_rsl([OsString::from("--"), OsString::from("--rules")]),
            err(displays_as(contains_substring("could not read Rust source")))
        );
    }

    #[test]
    fn test_rsl_when_rule_is_unknown_lists_kebab_case_ids() {
        assert_that!(
            run_rsl([OsString::from("--rules"), OsString::from("unknown")]),
            err(displays_as(contains_substring(
                "available_rules=misordered-item-group, misordered-visibility, nonadjacent-impl, misordered-fn, unqualified-call, overqualified-call, qualified-item, aliased-import, relative-path"
            )))
        );
    }

    #[test]
    fn test_rsl_when_relative_call_uses_super_reports_relative_path() {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "sample.rs",
            r"
            mod parent {
                mod child {
                    fn run() {
                        super::helper();
                    }
                }
                fn helper() {}
            }
            ",
        ));

        let expected_file = source.to_string_lossy().into_owned();
        let output = require(run_rsl(vec![source.into_os_string()]));

        assert_that!(output, eq(format!("{expected_file}:5:25,use a crate-absolute path\n")));
    }

    #[test]
    fn test_rsl_when_qualified_result_paths_are_allowed_returns_no_output() {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(
            &directory,
            "sample.rs",
            r"
            fn inspect(_: std::fmt::Result) -> rootcause::Result<()> {
                panic!()
            }
            ",
        ));

        assert_that!(run_rsl(vec![source.into_os_string()]), ok(eq(String::new())));
    }

    #[rstest::rstest]
    #[case("--json")]
    #[case("--rule")]
    fn test_rsl_when_unknown_option_is_supplied_returns_usage_error(#[case] option: &str) {
        let directory = require(tempfile::tempdir());
        let source = require(write_source(&directory, "sample.rs", "fn main() {}"));

        assert_that!(
            run_rsl(vec![OsString::from(option), source.into_os_string()]),
            err(displays_as(contains_substring("unknown rsl option")))
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
        let output = crate::cmds::rsl::run(Arguments::from_vec(arguments.into_iter().collect()))?;
        if output.is_empty() {
            return Ok(String::new());
        }

        Ok(format!("{}\n", output.render()))
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
