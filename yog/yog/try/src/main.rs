//! Re-run a command until success (ok) or failure (ko) with cooldown.
//!
//! # Errors
//! - Argument parsing or command execution fails.
#![feature(exit_status_error)]

use core::str::FromStr;
use std::process::Command;
use std::process::ExitStatusError;
use std::time::Duration;
use std::time::Instant;

use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_sys::cli::Args;

/// Exit condition for retry loop.
#[cfg_attr(test, derive(Debug))]
enum ExitCond {
    /// Exit when the command succeeds.
    Ok,
    /// Exit when the command fails.
    Ko,
}

impl ExitCond {
    /// Determines if the loop should break based on the exit condition and command result.
    pub const fn should_break(&self, cmd_res: Result<(), ExitStatusError>) -> bool {
        matches!((self, cmd_res), (Self::Ok, Ok(())) | (Self::Ko, Err(_)))
    }
}

/// Parses [`ExitCond`] from string.
impl FromStr for ExitCond {
    type Err = rootcause::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "ok" => Self::Ok,
            "ko" => Self::Ko,
            unexpected => Err(report!("unexpected exit condition")).attach_with(|| format!("value={unexpected}"))?,
        })
    }
}

/// Re-run a command until success (ok) or failure (ko) with cooldown.
#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let Some((cooldown_secs, args)) = args.split_first() else {
        return Err(report!("missing cooldown arg")).attach_with(|| format!("args={args:#?}"));
    };
    let cooldown = Duration::from_secs(
        cooldown_secs
            .parse()
            .context("invalid cooldown secs")
            .attach_with(|| format!("value={cooldown_secs}"))?,
    );

    let Some((exit_cond, args)) = args.split_first() else {
        return Err(report!("missing exit condition arg")).attach_with(|| format!("args={args:#?}"));
    };
    let exit_cond = ExitCond::from_str(exit_cond)
        .context("invalid exit condition")
        .attach_with(|| format!("args={args:#?}"))?;

    let Some((program, program_args)) = args.split_first() else {
        return Err(report!("missing command arg")).attach_with(|| format!("args={args:#?}"));
    };

    let mut tries = vec![];
    loop {
        let now = Instant::now();
        let output = Command::new(program)
            .args(program_args)
            .output()
            .context("error running cmd")
            .attach_with(|| format!("program={program:?} args={program_args:?}"))?;
        tries.push(now.elapsed());

        let terminal_output = if output.status.success() {
            output.stdout
        } else {
            output.stderr
        };
        println!("{}", String::from_utf8_lossy(&terminal_output));

        if exit_cond.should_break(output.status.exit_ok()) {
            break;
        }
        std::thread::sleep(cooldown);
    }

    let tries_count = u32::try_from(tries.len())
        .context("cannot convert tries len to u32")
        .attach_with(|| format!("len={}", tries.len()))?;
    let total_time = tries.iter().fold(Duration::ZERO, |acc, &d| acc.saturating_add(d));
    let avg_runs_time = if tries_count > 0 {
        total_time.checked_div(tries_count).unwrap_or(Duration::ZERO)
    } else {
        Duration::ZERO
    };
    println!("Summary:\n - tries {tries_count}\n - avg time {avg_runs_time:#?}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use super::*;

    #[test]
    fn exit_cond_from_str_when_ok_returns_ok_variant() {
        assert2::assert!(let Ok(ExitCond::Ok) = ExitCond::from_str("ok"));
    }

    #[test]
    fn exit_cond_from_str_when_ko_returns_ko_variant() {
        assert2::assert!(let Ok(ExitCond::Ko) = ExitCond::from_str("ko"));
    }

    #[test]
    fn exit_cond_from_str_when_invalid_returns_error() {
        assert2::assert!(let Err(err) = ExitCond::from_str("invalid"));
        assert!(err.to_string().contains("unexpected exit condition"));
    }

    #[test]
    fn should_break_ok_cond_with_success_result_returns_true() {
        pretty_assertions::assert_eq!(ExitCond::Ok.should_break(Ok(())), true);
    }

    #[test]
    fn should_break_ok_cond_with_failure_result_returns_false() {
        let err_result: Result<(), ExitStatusError> = std::process::Command::new("false").status().unwrap().exit_ok();
        pretty_assertions::assert_eq!(ExitCond::Ok.should_break(err_result), false);
    }

    #[test]
    fn should_break_ko_cond_with_failure_result_returns_true() {
        let err_result: Result<(), ExitStatusError> = std::process::Command::new("false").status().unwrap().exit_ok();
        pretty_assertions::assert_eq!(ExitCond::Ko.should_break(err_result), true);
    }

    #[test]
    fn should_break_ko_cond_with_success_result_returns_false() {
        pretty_assertions::assert_eq!(ExitCond::Ko.should_break(Ok(())), false);
    }
}
