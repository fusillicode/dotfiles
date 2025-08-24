#![feature(exit_status_error)]

use std::process::Command;
use std::process::ExitStatusError;
use std::str::FromStr;
use std::time::Duration;
use std::time::Instant;

use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;

/// Repeatedly executes a command until it meets the specified exit condition.
///
/// This tool is designed for scenarios where you need to wait for a command to succeed
/// or fail, such as waiting for a service to start, a file to become available, or
/// a network connection to be established. It will keep running the command with
/// a configurable cooldown period until the exit condition is met.
///
/// # Arguments
///
/// * `cooldown_secs` - Number of seconds to wait between command executions
/// * `exit_condition` - Either "ok" (stop on success) or "ko" (stop on failure)
/// * `command` - The command to execute (can include arguments and shell features)
///
/// # Exit Conditions
///
/// - "ok": Stop when the command returns success (exit code 0)
/// - "ko": Stop when the command returns failure (non-zero exit code)
///
/// # Output
///
/// - Successful command output is printed to stdout
/// - Failed command output is printed to stderr
/// - Final summary shows total tries and average execution time
///
/// # Examples
///
/// Wait for a service to start (stop on success):
/// ```bash
/// try 5 ok "curl -f http://localhost:3000/health"
/// ```
///
/// Wait for a process to fail (stop on failure):
/// ```bash
/// try 2 ko "pg_isready -h localhost -p 5432"
/// ```
///
/// Retry a flaky command:
/// ```bash
/// try 1 ok "npm test -- --watchAll=false"
/// ```
///
/// # Use Cases
///
/// - Waiting for services to be ready during deployment
/// - Polling for file availability
/// - Testing network connectivity
/// - Retrying flaky operations
/// - Monitoring process health
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = utils::system::get_args();

    let Some((cooldown_secs, args)) = args.split_first() else {
        bail!("no cooldown supplied in {args:#?}");
    };
    let cooldown = Duration::from_secs(
        cooldown_secs
            .parse()
            .with_context(|| format!("cannot parse {cooldown_secs} into Duration"))?,
    );

    let Some((exit_cond, args)) = args.split_first() else {
        bail!("no exit condition supplied in {args:#?}");
    };
    let exit_cond = ExitCond::from_str(exit_cond).with_context(|| format!("in supplied args {args:#?}"))?;

    let cmd = args.join(" ");

    let mut tries = vec![];
    loop {
        let now = Instant::now();
        let output = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .with_context(|| cmd.to_string())?;
        tries.push(now.elapsed());

        let terminal_output = if output.status.success() {
            output.stdout
        } else {
            output.stderr
        };
        println!("{}", String::from_utf8_lossy(&terminal_output));

        if exit_cond.should_break(&output.status.exit_ok()) {
            break;
        }
        std::thread::sleep(cooldown);
    }

    let tries_count = u32::try_from(tries.len()).with_context(|| format!("converting {} to u32", tries.len()))?;
    let avg_runs_time = tries.iter().fold(Duration::ZERO, |acc, &d| acc + d) / tries_count;
    println!("Summary:\n - tries {tries_count}\n - avg time {avg_runs_time:#?}");

    Ok(())
}

/// Represents the condition under which the retry loop should exit.
///
/// This enum defines the two possible exit conditions for the retry mechanism:
/// - `Ok`: Exit when the command succeeds (returns exit code 0)
/// - `Ko`: Exit when the command fails (returns non-zero exit code)
enum ExitCond {
    /// Exit when the command succeeds.
    Ok,
    /// Exit when the command fails.
    Ko,
}

impl ExitCond {
    /// Determines if the loop should break based on the command result and the exit condition.
    pub fn should_break(&self, cmd_res: &Result<(), ExitStatusError>) -> bool {
        self.is_ok() && cmd_res.is_ok() || !self.is_ok() && !cmd_res.is_ok()
    }

    /// Checks if this exit condition represents success.
    ///
    /// # Returns
    ///
    /// Returns `true` if this is the `Ok` variant (exit on success),
    /// `false` if this is the `Ko` variant (exit on failure).
    fn is_ok(&self) -> bool {
        match self {
            ExitCond::Ok => true,
            ExitCond::Ko => false,
        }
    }
}

/// Parses an [ExitCond] from a string representation.
impl FromStr for ExitCond {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "ok" => ExitCond::Ok,
            "ko" => ExitCond::Ko,
            unexpected => bail!("unexpected exit condition value {unexpected}"),
        })
    }
}
