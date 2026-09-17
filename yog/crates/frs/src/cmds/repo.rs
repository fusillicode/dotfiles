//! Implementation of the `frs repo` command.

use rootcause::report;
use ytil_sys::pico_args::Arguments;

mod fix;

/// Runs the `frs repo` command.
///
/// # Errors
/// - The subcommand is unknown or its arguments are invalid.
/// - Repo maintenance fails.
pub fn run(mut cli_args: Arguments) -> rootcause::Result<()> {
    let command = cli_args.subcommand()?;
    match command.as_deref() {
        None => {
            if cli_args.contains("--help") {
                print!("{}", crate::cmds::Help::Repo.text());
                return Ok(());
            }
            if cli_args.finish().is_empty() {
                eprintln!("{}", crate::cmds::Help::Repo.text());
                return Err(report!("missing frs repo command"));
            }
            Err(report!("missing repo subcommand"))
        }
        Some("fix") => fix::run(cli_args),
        Some(command) => Err(report!("unsupported repo command").attach(format!("command={command}"))),
    }
}
