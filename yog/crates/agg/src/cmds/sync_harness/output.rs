//! Format sync outcomes at the CLI boundary. Successful outcomes use stdout;
//! conflicts and skipped subagents use stderr with the CLI warning color.
//! Changed entries use green labels; unchanged entries use cyan labels.

use std::io::Write;

use owo_colors::OwoColorize;

use crate::cmds::sync_harness::common::SyncOutcome;

pub(super) fn write(
    outcomes: &[SyncOutcome],
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> rootcause::Result<()> {
    for event in outcomes {
        match event {
            SyncOutcome::Linked { source, destination } => {
                writeln!(
                    stdout,
                    "{} {} -> {}",
                    "linked".green().bold(),
                    destination.display(),
                    source.display()
                )?;
            }
            SyncOutcome::Generated { destination } => {
                writeln!(stdout, "{} {}", "generated".green().bold(), destination.display())?;
            }
            SyncOutcome::Unchanged { destination } => {
                writeln!(stdout, "{} {}", "already synced".cyan().bold(), destination.display())?;
            }
            SyncOutcome::Conflict { destination, operation } => {
                stdout.flush()?;
                writeln!(
                    stderr,
                    "{} {} already exists; {operation} skipped",
                    "warning:".yellow().bold(),
                    destination.display()
                )?;
            }
            SyncOutcome::SubagentSkipped { error } => {
                stdout.flush()?;
                writeln!(stderr, "{} subagent skipped\n{error}", "warning:".yellow().bold())?;
            }
        }
    }

    Ok(())
}
