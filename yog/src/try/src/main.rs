#![feature(exit_status_error)]

use core::str::FromStr;
use std::process::Command;
use std::process::ExitStatusError;
use std::time::Duration;
use std::time::Instant;

use color_eyre::eyre;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;

/// Repeatedly executes a command until exit condition is met.
///
/// # Arguments
///
/// * `cooldown_secs` - Seconds to wait between executions
/// * `exit_condition` - "ok" (stop on success) or "ko" (stop on failure)
/// * `command` - Command to execute
///
/// # Examples
///
/// ```bash
/// try 5 ok "curl -f http://localhost:3000/health"
/// try 2 ko "pg_isready -h localhost -p 5432"
/// ```
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = utils::system::get_args();

    let Some((cooldown_secs, args)) = args.split_first() else {
        bail!("missing cooldown supplied in {args:#?}");
    };
    let cooldown = Duration::from_secs(
        cooldown_secs
            .parse()
            .with_context(|| format!("cannot parse {cooldown_secs} into Duration"))?,
    );

    let Some((exit_cond, args)) = args.split_first() else {
        bail!("missing exit condition supplied in {args:#?}");
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
            .with_context(|| cmd.clone())?;
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

    let tries_count = u32::try_from(tries.len()).with_context(|| format!("converting {} to u32", tries.len()))?;
    let total_time = tries.iter().fold(Duration::ZERO, |acc, &d| acc.saturating_add(d));
    let avg_runs_time = if tries_count > 0 {
        total_time.checked_div(tries_count).unwrap_or(Duration::ZERO)
    } else {
        Duration::ZERO
    };
    println!("Summary:\n - tries {tries_count}\n - avg time {avg_runs_time:#?}");

    Ok(())
}

/// Exit condition for retry loop.
enum ExitCond {
    /// Exit when the command succeeds.
    Ok,
    /// Exit when the command fails.
    Ko,
}

impl ExitCond {
    /// Determines if the loop should break.
    #[allow(clippy::suspicious_operation_groupings)]
    pub const fn should_break(&self, cmd_res: Result<(), ExitStatusError>) -> bool {
        self.is_ok() && cmd_res.is_ok() || !self.is_ok() && cmd_res.is_err()
    }

    /// Checks if this represents success.
    const fn is_ok(&self) -> bool {
        match self {
            Self::Ok => true,
            Self::Ko => false,
        }
    }
}

/// Parses [`ExitCond`] from string.
impl FromStr for ExitCond {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "ok" => Self::Ok,
            "ko" => Self::Ko,
            unexpected => bail!("unexpected exit condition value {unexpected}"),
        })
    }
}
