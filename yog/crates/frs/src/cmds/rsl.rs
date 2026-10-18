//! The `frs rsl` command and its command-line interface.

use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;

use rootcause::report;
use ytil_sys::pico_args::Arguments;

pub use self::output::RslOutput;
use self::rules::SelectedRules;

mod analysis;
mod ast;
mod engine;
mod output;
mod rules;
mod source;

/// Runs `frs rsl`.
///
/// Writes safe fixes and reports remaining findings, or reports without changing files.
///
/// # Errors
///
/// Returns an error when arguments are invalid or a file cannot be read, parsed, or written.
pub fn run(mut cli_args: Arguments) -> rootcause::Result<RslOutput> {
    if cli_args.contains("--help") {
        print!("{}", crate::cmds::Help::Rsl.text());
        return Ok(RslOutput::default());
    }

    let opts = match RslOpts::try_from(cli_args.finish()) {
        Ok(opts) => opts,
        Err(error) => {
            eprintln!("{}", crate::cmds::Help::Rsl.text());
            return Err(error);
        }
    };

    let reports = self::engine::process_paths(&opts.paths, &opts.rules, opts.mode)?;

    Ok(RslOutput { reports })
}

#[derive(Clone, Copy, Default)]
enum Mode {
    #[default]
    Write,
    Report,
}

impl FromStr for Mode {
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "write" => Ok(Self::Write),
            "report" => Ok(Self::Report),
            _ => Err(report!("unknown rsl mode")
                .attach(format!("mode={value}"))
                .attach("available_modes=write, report")),
        }
    }
}

struct RslOpts {
    paths: Vec<PathBuf>,
    rules: SelectedRules,
    mode: Mode,
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
        let mode = cli_args.opt_value_from_str("--mode")?.unwrap_or_default();
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
            paths: paths.into_iter().map(PathBuf::from).collect(),
            rules,
            mode,
        })
    }
}
